//! Account Mesh composition: registry, protected key, lifecycle state, transport and local facade.

use super::account::AccountCell;
use super::ServiceContext;
use async_trait::async_trait;
use codeterm_account::account_mesh::callable::{CallableRegistry, HttpCallable};
use codeterm_account::account_mesh::wire::{
    MeshEndpoint, MeshTarget, RevokedResult, UpdatedResult,
};
use codeterm_account::account_mesh::{
    AccountMesh, AccountMeshConfig, AccountMeshPorts, AccountMeshRecord, AccountMeshRefusal,
    AccountMeshStore, AccountMeshView, EndpointSource, EnrollTrigger, FileAccountMeshStore,
    LifecycleState, MembershipTrust, MeshRegistry, MeshTransport, Presence, SessionSource,
    SystemClock, UnavailableRegistry,
};
use codeterm_account::Session;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use tokio::sync::{watch, OnceCell};

mod authority;
mod disconnect;
mod link_approval;
mod presence;
pub mod tailnet;
pub use authority::{mesh_state, AccountAuthority};

#[cfg(test)]
mod tests;

pub const ACCOUNT_MESH_CHANGED: &str = "account-mesh-changed";

#[derive(Default)]
pub struct AccountMeshCell(OnceCell<Arc<AccountMeshService>>);

pub struct AccountMeshService {
    pub mesh: Arc<AccountMesh>,
    listener: Arc<ListenerEndpoint>,
    mesh_state: watch::Sender<codeterm_mesh::account_mesh::AccountMeshState>,
    restored: AtomicBool,
}

impl AccountMeshService {
    /// Latest account state for the Mesh gate; every account transition replaces it.
    pub fn mesh_states(&self) -> watch::Receiver<codeterm_mesh::account_mesh::AccountMeshState> {
        self.mesh_state.subscribe()
    }

    pub fn authority(&self) -> Arc<AccountAuthority> {
        Arc::new(AccountAuthority(self.mesh.clone()))
    }
}

struct AccountSessions(Arc<OnceCell<Arc<codeterm_account::Account>>>);
fn session_refusal(error: codeterm_account::Error) -> AccountMeshRefusal {
    match error {
        codeterm_account::Error::Credentials => AccountMeshRefusal::Unauthenticated,
        _ => AccountMeshRefusal::Unavailable,
    }
}
#[async_trait]
impl SessionSource for AccountSessions {
    async fn session(&self) -> Result<Option<Session>, AccountMeshRefusal> {
        let account = self.0.get().ok_or(AccountMeshRefusal::Unauthenticated)?;
        account.fresh_session().await.map_err(session_refusal)
    }
}

#[derive(Default)]
struct ListenerEndpoint(Mutex<Option<MeshEndpoint>>);
impl EndpointSource for ListenerEndpoint {
    fn ready_endpoint(&self) -> Option<MeshEndpoint> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

struct TransportBridge {
    service: Weak<ServiceContext>,
    mesh_state: watch::Sender<codeterm_mesh::account_mesh::AccountMeshState>,
}
#[async_trait]
impl MeshTransport for TransportBridge {
    async fn presence_changed(&self, metadata_revision: u64) {
        presence::notify(&self.service, metadata_revision).await;
    }
    async fn apply(&self, state: &LifecycleState) {
        let projected = mesh_state(state);
        if let Some(service) = self.service.upgrade() {
            if let Some(mesh) = service.try_ext::<crate::MeshStateHandle>().cloned() {
                mesh.lock().apply_account_state(projected.clone());
            }
            service.ui.emit(
                ACCOUNT_MESH_CHANGED,
                serde_json::json!({ "account_epoch": state.account_epoch }),
            );
        }
        publish_mesh_state(&self.mesh_state, projected);
    }
    // Revocations reach the gate through `revoked_installations` in the pushed state.
    async fn installation_revoked(&self, _installation_id: &str) {}
}

/// `account_mesh_state`: account-owned fields plus the transport's saved pairs.
#[derive(Clone, Debug, Serialize)]
pub struct AccountMeshSnapshot {
    #[serde(flatten)]
    pub view: AccountMeshView,
    pub pairs: Vec<serde_json::Value>,
    pub link_requests: Vec<codeterm_mesh::account_mesh::approval::LinkRequest>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum AccountMeshOutcome<T> {
    Done {
        ok: bool,
        result: T,
    },
    Refused {
        ok: bool,
        request_id: Option<String>,
        failure: AccountMeshRefusal,
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        local_signed_out: bool,
    },
}

impl<T> AccountMeshOutcome<T> {
    pub fn of(result: Result<T, AccountMeshRefusal>, request_id: Option<String>) -> Self {
        match result {
            Ok(result) => Self::Done { ok: true, result },
            Err(failure) => Self::Refused {
                ok: false,
                request_id,
                failure,
                local_signed_out: false,
            },
        }
    }
}

fn trust() -> Option<MembershipTrust> {
    Some(MembershipTrust::new(codeterm_entitlements::pinned()))
}

impl ServiceContext {
    fn registry(&self) -> Arc<dyn MeshRegistry> {
        let configured = self
            .ext::<crate::config::ConfigState>()
            .lock()
            .billing
            .get("endpoint")
            .and_then(|value| value.as_str())
            .map(str::to_owned);
        let endpoint =
            match codeterm_account::firebase_endpoints::configured_functions(configured.as_deref())
            {
                Ok(endpoint) => endpoint,
                Err(message) => {
                    log::error!("{message}");
                    return Arc::new(UnavailableRegistry);
                }
            };
        match endpoint.map(HttpCallable::new) {
            Some(Ok(transport)) => Arc::new(
                CallableRegistry::new(transport).with_revocations(self.session_revocations()),
            ),
            _ => Arc::new(UnavailableRegistry),
        }
    }

    pub async fn account_mesh(&self) -> Result<Arc<AccountMeshService>, String> {
        let cell = self
            .extensions
            .get_or_init::<AccountMeshCell>(AccountMeshCell::default);
        let service = cell
            .0
            .get_or_try_init(|| async {
                let installation_id = self.installation_id().await?;
                let listener = Arc::new(ListenerEndpoint::default());
                let mesh_state = watch::channel(Default::default()).0;
                let mesh = AccountMesh::new(
                    AccountMeshConfig {
                        installation_id,
                        default_display_name: Some(self.configuration().mesh.instance_name.clone()),
                        trust: trust(),
                        call_deadline: Duration::from_secs(15),
                        withdrawal_deadline: Duration::from_secs(10),
                        schedule_leases: true,
                    },
                    AccountMeshPorts {
                        registry: self.registry(),
                        sessions: Arc::new(AccountSessions(self.ext::<AccountCell>().0.clone())),
                        keys: self.account_credentials().await?,
                        store: Arc::new(FileAccountMeshStore::new(self.account_data_root()?)),
                        endpoint: listener.clone(),
                        transport: Arc::new(TransportBridge {
                            service: self.weak_self(),
                            mesh_state: mesh_state.clone(),
                        }),
                        clock: Arc::new(SystemClock),
                    },
                );
                mesh.load().await;
                Ok::<_, String>(Arc::new(AccountMeshService {
                    mesh,
                    listener,
                    mesh_state,
                    restored: AtomicBool::new(false),
                }))
            })
            .await?;
        Ok(service.clone())
    }

    /// Signed-in restore enrolls once per process; later `account()` calls are cheap.
    pub(crate) fn account_mesh_restored(&self) {
        let Some(service) = self
            .extensions
            .try_get::<AccountMeshCell>()
            .and_then(|cell| cell.0.get())
        else {
            return;
        };
        if service.restored.swap(true, Ordering::AcqRel) {
            return;
        }
        if matches!(service.mesh.state().presence, Presence::SignedIn { .. }) {
            self.spawn_account_mesh_refresh(EnrollTrigger::Automatic);
        }
    }

    /// Enrollment runs beside sign-in; its outcome never changes the sign-in result.
    pub fn spawn_account_mesh_refresh(&self, trigger: EnrollTrigger) {
        let Some(service) = self
            .extensions
            .try_get::<AccountMeshCell>()
            .and_then(|cell| cell.0.get())
            .cloned()
        else {
            return;
        };
        tokio::spawn(async move {
            let _ = service.mesh.refresh(trigger).await;
        });
    }

    /// A plan change may lift the machine limit: re-enroll so a locked machine reopens live.
    pub(crate) fn unlock_account_mesh_after_plan_change(&self) {
        let locked = self
            .extensions
            .try_get::<AccountMeshCell>()
            .and_then(|cell| cell.0.get())
            .is_some_and(|service| service.mesh.state().plan_limit_locked());
        if locked {
            self.spawn_account_mesh_refresh(EnrollTrigger::Lease);
        }
    }

    /// Transport reports the listener once it is reachable from the Tailnet, or `None`.
    pub async fn set_account_mesh_listener(&self, endpoint: Option<MeshEndpoint>) {
        let Ok(service) = self.account_mesh().await else {
            return;
        };
        let changed = {
            let mut current = service
                .listener
                .0
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let changed = *current != endpoint;
            *current = endpoint.clone();
            changed
        };
        if changed {
            self.spawn_account_mesh_refresh(EnrollTrigger::Network);
        }
    }

    pub async fn account_mesh_snapshot(&self) -> Result<AccountMeshSnapshot, String> {
        let service = self.account_mesh().await?;
        Ok(AccountMeshSnapshot {
            view: service.mesh.state().view(),
            pairs: self.account_mesh_pairs(),
            link_requests: self
                .try_ext::<crate::MeshStateHandle>()
                .map(|mesh| mesh.lock().link_requests())
                .unwrap_or_default(),
        })
    }

    pub(super) async fn sign_out_without_account_mesh(&self) -> Result<(), String> {
        let mesh = self.try_ext::<crate::MeshStateHandle>().cloned();
        let store: Arc<dyn AccountMeshStore> =
            Arc::new(FileAccountMeshStore::new(crate::paths::data_dir()));
        block_and_persist_sign_out(mesh, store).await
    }

    /// Saved pairs with their admission, joined from the Mesh transport.
    fn account_mesh_pairs(&self) -> Vec<serde_json::Value> {
        let Some(mesh) = self.try_ext::<crate::MeshStateHandle>() else {
            return Vec::new();
        };
        let pairs = mesh.lock().account_mesh_pairs();
        pairs
            .into_iter()
            .filter_map(|pair| serde_json::to_value(pair).ok())
            .collect()
    }

    /// Binds the Mesh gate to the account lifecycle and the ready listener to enrollment;
    /// each account state reaches the gate before adoption runs.
    pub fn start_account_mesh_transport(&self) {
        let Some(service) = self.weak_self().upgrade() else {
            return;
        };
        crate::async_runtime::spawn(async move {
            let Some(mesh) = service.try_ext::<crate::MeshStateHandle>().cloned() else {
                log::error!("[account-mesh] no Mesh state; Mesh traffic stays blocked");
                return;
            };
            crate::async_runtime::spawn(tailnet::run(service.clone(), mesh.clone()));
            let account_mesh = match service.account_mesh().await {
                Ok(account_mesh) => account_mesh,
                Err(error) => {
                    log::warn!(
                        "[account-mesh] unavailable; gating on the persisted sign-out: {error}"
                    );
                    let persisted = FileAccountMeshStore::new(crate::paths::data_dir())
                        .load()
                        .await;
                    mesh.lock()
                        .apply_account_state(fallback_mesh_state(persisted));
                    return;
                }
            };
            mesh.lock().bind_account_authority(account_mesh.authority());
            crate::async_runtime::spawn(push_account_mesh_listener(service.clone(), mesh.clone()));
            let mut states = account_mesh.mesh_states();
            loop {
                let state = states.borrow_and_update().clone();
                mesh.lock().apply_account_state(state);
                crate::async_runtime::spawn(
                    codeterm_mesh::account_mesh::link::on_account_state_applied(mesh.clone()),
                );
                if states.changed().await.is_err() {
                    return;
                }
            }
        });
    }

    pub async fn account_mesh_enroll(&self) -> Result<AccountMeshOutcome<()>, String> {
        let service = self.account_mesh().await?;
        let result = service.mesh.refresh(EnrollTrigger::Explicit).await;
        let request_id = service
            .mesh
            .state()
            .limit_notice
            .as_ref()
            .filter(|notice| result == Err(notice.refusal))
            .map(|notice| notice.request_id.clone());
        Ok(AccountMeshOutcome::of(result, request_id))
    }

    pub async fn account_mesh_list(&self) -> Result<AccountMeshOutcome<()>, String> {
        let service = self.account_mesh().await?;
        Ok(AccountMeshOutcome::of(
            service.mesh.list().await.map(|_| ()),
            None,
        ))
    }

    pub async fn account_mesh_rename_self(
        &self,
        expected_metadata_revision: u64,
        display_name: String,
    ) -> Result<AccountMeshOutcome<UpdatedResultView>, String> {
        let service = self.account_mesh().await?;
        let result = service
            .mesh
            .rename_self(expected_metadata_revision, display_name)
            .await
            .map(UpdatedResultView::from);
        Ok(AccountMeshOutcome::of(result, None))
    }

    pub async fn account_mesh_revoke(
        &self,
        target: MeshTarget,
    ) -> Result<AccountMeshOutcome<RevokedResultView>, String> {
        let service = self.account_mesh().await?;
        let result = service
            .mesh
            .revoke(target)
            .await
            .map(RevokedResultView::from);
        Ok(AccountMeshOutcome::of(result, None))
    }

    pub async fn account_mesh_session_revoke(
        &self,
        installation_id: Option<String>,
    ) -> Result<
        AccountMeshOutcome<codeterm_account::account_mesh::wire::SessionRevokedResult>,
        String,
    > {
        let self_id = self.installation_id().await?;
        let installation_id = installation_id.unwrap_or_else(|| self_id.as_str().to_owned());
        let own = installation_id == self_id.as_str();
        if own {
            let service = self.clone();
            let (_, result) = tokio::spawn(async move { service.account_sign_out_owned().await })
                .await
                .map_err(|_| codeterm_account::Error::Cancelled.to_string())??;
            let result = result.and_then(|result| result.ok_or(AccountMeshRefusal::Unavailable));
            return Ok(match result {
                Ok(result) => AccountMeshOutcome::of(Ok(result), None),
                Err(failure) => {
                    log::warn!("Account sign-out cleanup failed: stage=session_revoke error_kind={failure:?}");
                    AccountMeshOutcome::Refused {
                        ok: false,
                        request_id: None,
                        failure,
                        local_signed_out: true,
                    }
                }
            });
        }
        let service = self.account_mesh().await?;
        let _ = self.account().await?;
        let result = service.mesh.revoke_session(installation_id).await;
        Ok(AccountMeshOutcome::of(result, None))
    }

    pub async fn account_mesh_acknowledge_limit(&self, request_id: &str) -> Result<bool, String> {
        Ok(self
            .account_mesh()
            .await?
            .mesh
            .acknowledge_limit_notice(request_id)
            .await)
    }
}

async fn block_and_persist_sign_out(
    mesh: Option<crate::MeshStateHandle>,
    store: Arc<dyn AccountMeshStore>,
) -> Result<(), String> {
    if let Some(mesh) = mesh {
        let mut manager = mesh.lock();
        let account_epoch = manager.account_state().account_epoch.saturating_add(1);
        manager.apply_account_state(codeterm_mesh::account_mesh::AccountMeshState {
            account_epoch,
            presence: codeterm_mesh::account_mesh::AccountPresence::SignedOut,
            ..codeterm_mesh::account_mesh::AccountMeshState::default()
        });
    }
    let mut record = store.load().await.unwrap_or_default();
    record.signed_out = true;
    store
        .save(&record)
        .await
        .map_err(|_| "Account Mesh sign-out could not be persisted".to_string())
}

/// Wakes the gate only when the projection changes, so lifecycle pushes cannot storm announces.
fn publish_mesh_state(
    sender: &watch::Sender<codeterm_mesh::account_mesh::AccountMeshState>,
    next: codeterm_mesh::account_mesh::AccountMeshState,
) -> bool {
    sender.send_if_modified(|current| {
        if *current == next {
            return false;
        }
        *current = next;
        true
    })
}

/// Gate state without a running account service: never open past a persisted or unreadable sign-out.
fn fallback_mesh_state(
    persisted: std::io::Result<AccountMeshRecord>,
) -> codeterm_mesh::account_mesh::AccountMeshState {
    use codeterm_mesh::account_mesh::{AccountMeshState, AccountPresence};
    match persisted {
        Ok(record) if !record.signed_out => AccountMeshState {
            presence: AccountPresence::NeverSignedIn,
            revoked_installations: record.revoked_installations,
            ..AccountMeshState::default()
        },
        _ => AccountMeshState {
            presence: AccountPresence::SignedOut,
            ..AccountMeshState::default()
        },
    }
}

async fn push_account_mesh_listener(service: Arc<ServiceContext>, mesh: crate::MeshStateHandle) {
    let mut changes = mesh.lock().gate_watch();
    let mut published: Option<Option<MeshEndpoint>> = None;
    loop {
        let endpoint = {
            let manager = mesh.lock();
            let identity = manager
                .network_transport
                .as_ref()
                .and_then(|status| status.identity.as_ref())
                .map(
                    |identity| codeterm_account::account_mesh::wire::NetworkIdentity {
                        kind: identity.kind.clone(),
                        network_id: identity.network_id.clone(),
                    },
                );
            manager
                .ready_tailnet_endpoint()
                .and_then(|(ip, port)| MeshEndpoint::new(ip, port))
                .map(|endpoint| endpoint.with_transport(identity))
        };
        if published.as_ref() != Some(&endpoint) {
            service.set_account_mesh_listener(endpoint.clone()).await;
            published = Some(endpoint);
        }
        if changes.changed().await.is_err() {
            return;
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UpdatedResultView {
    pub display_name: String,
    pub metadata_revision: u64,
}
impl From<UpdatedResult> for UpdatedResultView {
    fn from(value: UpdatedResult) -> Self {
        Self {
            display_name: value.display_name,
            metadata_revision: value.metadata_revision,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RevokedResultView {
    pub installation_id: String,
    pub slot_revision: u64,
}
impl From<RevokedResult> for RevokedResultView {
    fn from(value: RevokedResult) -> Self {
        Self {
            installation_id: value.target_installation_id,
            slot_revision: value.slot_revision,
        }
    }
}
