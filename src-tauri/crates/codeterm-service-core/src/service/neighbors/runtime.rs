mod authority;
pub(super) mod callable;
mod concierge;
pub(super) mod files;
mod gateway;
mod listener;
mod local;
pub(crate) mod logout;
mod owner;
mod pending;
mod permission;
mod read;
mod receiver;
mod restoration;
mod send;
mod snapshots;

use super::storage::Metadata;
use crate::service::ServiceContext;
use codeterm_neighbors::{
    adapter::{FirebaseRegistry, FirebaseTransport},
    keys::{InstallationKeys, KeyRegistry, KeyScope, OsKeyStore},
    permissions::Permission,
    receive::{CursorStore, NeighborTransport, Receiver},
    wire::{hex_id, EncryptionKeyIds, Envelope, Kind, Timestamp, SCHEMA},
    Error,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{collections::HashMap, sync::Arc};
use zeroize::Zeroizing;

#[derive(Default)]
pub struct NeighborRuntimeCell {
    pub(super) handling: super::handling::Cell,
    pub(super) launches: Arc<super::launch_fence::State>,
    pub(super) pauses: super::pause::State,
    pub(super) concierge_accounts: parking_lot::Mutex<HashMap<String, String>>,
    pub(super) restore_grants:
        parking_lot::Mutex<HashMap<String, super::confinement::RestoreGrant>>,
    registry_snapshot: parking_lot::Mutex<Option<Value>>,
    owner: parking_lot::Mutex<Option<tokio::sync::mpsc::Sender<owner::Request>>>,
    pub(super) generation: AtomicU64,
    listener_generation: AtomicU64,
    listeners: parking_lot::Mutex<listener::Slots>,
    pub(super) states:
        parking_lot::Mutex<HashMap<String, codeterm_neighbors::supervisor::ListenerState>>,
    pub(super) failures: parking_lot::Mutex<HashMap<String, Value>>,
    pending_counts: parking_lot::Mutex<HashMap<String, u64>>,
    names: parking_lot::Mutex<HashMap<String, String>>,
    receiver: parking_lot::Mutex<Option<codeterm_neighbors::adapter::ReceiverStatus>>,
    control: parking_lot::Mutex<Option<tokio::task::JoinHandle<()>>>,
    control_start: tokio::sync::Mutex<()>,
    control_cancel: tokio::sync::Notify,
    pub(super) watch_states: parking_lot::Mutex<std::collections::BTreeMap<String, Value>>,
    bootstrapped: std::sync::atomic::AtomicBool,
    restoration: parking_lot::Mutex<Option<tokio::task::JoinHandle<()>>>,
    file_advertised_generation: tokio::sync::Mutex<Option<u64>>,
}

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum NeighborRuntimeCommand {
    FlagMessage {
        friendship_id: String,
        message_id: String,
        reason: String,
        category: super::FlagCategory,
    },
    StopConcierge {
        friendship_id: String,
    },
    PauseFriend {
        friendship_id: String,
    },
    ResumeFriend {
        friendship_id: String,
    },
    NotificationPreference {
        friendship_id: String,
    },
    SetNotificationPreference {
        friendship_id: String,
        notify: super::NeighborNotify,
    },
    SetDailyLimit {
        friendship_id: String,
        daily_limit: Option<serde_json::Number>,
    },
    History {
        friendship_id: String,
        before: Option<String>,
        limit: Option<usize>,
    },
    ChangeAgent {
        friendship_id: String,
        provider: String,
        model: Option<String>,
        reasoning_effort: Option<String>,
        service_tier: Option<String>,
    },
    Status,
    Enroll {
        takeover_confirmed: bool,
        expected_receiver_revision: u64,
    },
    SetFriendReceiver {
        friendship_id: String,
        receiver_operation: receiver::selection::Selection,
        takeover_confirmed: bool,
        expected_receiver_revision: u64,
    },
    Rotate,
    Retry {
        friendship_id: String,
    },
    Revoke,
    OwnKeys,
    Bind {
        friendship_id: String,
        gateway_tab: String,
    },
    Permission {
        friendship_id: String,
    },
    SetPermission {
        friendship_id: String,
        permission: Value,
    },
    Send {
        friendship_id: String,
        kind: Kind,
        body: String,
        in_reply_to: Option<String>,
    },
    DeliveryStatus {
        message_id: String,
    },
}

struct Outgoing {
    friendship: String,
    kind: Kind,
    body: OutboundBody,
    in_reply_to: Option<String>,
    author: super::history::Author,
}

enum OutboundBody {
    Granted(Zeroizing<String>),
    Files {
        text: Zeroizing<String>,
        paths: Vec<String>,
    },
    Refusal,
}

pub(super) fn authoring_allowed(
    general: bool,
    authenticated: bool,
    expected: Option<&str>,
    observed: Option<&str>,
) -> bool {
    general && authenticated && expected.is_some() && expected == observed
}

impl ServiceContext {
    pub(super) fn neighbor_cell_names(
        &self,
    ) -> parking_lot::MutexGuard<'_, HashMap<String, String>> {
        self.neighbor_cell().names.lock()
    }
    pub(super) fn neighbor_history_generation(&self) -> u64 {
        self.neighbor_cell().generation.load(Ordering::Acquire)
    }

    pub(super) fn require_neighbor_history_generation(
        &self,
        generation: u64,
    ) -> codeterm_neighbors::Result<()> {
        self.require_neighbors_enabled()?;
        self.require_neighbor_generation(generation)
            .map_err(|_| Error::Revoked)
    }
    pub(super) fn neighbor_cell(&self) -> &NeighborRuntimeCell {
        self.extensions.get_or_init(NeighborRuntimeCell::default)
    }

    async fn neighbor_key_ports(
        &self,
    ) -> Result<(Arc<FirebaseRegistry>, KeyScope, Metadata), String> {
        let account = self.account().await?;
        if account.status().await.provider == "local" {
            return Err("Sign in with a hosted account to use Neighbors".into());
        }
        let session = account
            .current_session()
            .await
            .map_err(|_| "Sign in to use Neighbors")?
            .ok_or("Sign in to use Neighbors")?;
        let uid = session.identity.uid.clone();
        let installation_id = self.installation_id().await?.as_str().to_owned();
        let configured_label = self.configuration().mesh.instance_name.clone();
        let machine_label = if configured_label.trim().is_empty() {
            hostname::get()
                .ok()
                .and_then(|name| name.into_string().ok())
                .unwrap_or_else(|| "This machine".into())
        } else {
            configured_label
        };
        let registry = Arc::new(FirebaseRegistry {
            calls: Arc::new(callable::Calls {
                own_keys: Default::default(),
                receiver_status: Default::default(),
                service: self.clone(),
                uid: uid.clone(),
                generation: session.login_generation,
            }),
            uid: uid.clone(),
            installation_id: installation_id.clone(),
            machine_label,
            enrollment: None,
        });
        let scope = KeyScope::installation(self.installation_id().await?.as_str(), &uid)
            .map_err(|e| e.to_string())?;
        let metadata = Metadata::new(self.account_data_root()?, &uid);
        Ok((registry, scope, metadata))
    }

    async fn neighbor_ports(
        &self,
    ) -> Result<
        (
            Arc<FirebaseTransport>,
            Arc<FirebaseRegistry>,
            KeyScope,
            Metadata,
        ),
        String,
    > {
        let (registry, scope, metadata) = self.neighbor_key_ports().await?;
        self.retry_neighbor_logout(&metadata, &scope, &registry.uid, registry.as_ref())
            .await?;
        let transport = self.neighbor_transport(&registry)?;
        Ok((transport, registry, scope, metadata))
    }

    fn neighbor_transport(
        &self,
        registry: &FirebaseRegistry,
    ) -> Result<Arc<FirebaseTransport>, String> {
        let endpoints = codeterm_account::firebase_endpoints::current();
        if endpoints.emulator {
            return Err("Native Neighbor streaming requires the QA or production project".into());
        }
        let project = if endpoints.qa {
            "domios-signin-qa"
        } else {
            "domios-d9357"
        }
        .to_string();
        let transport = Arc::new(FirebaseTransport {
            calls: registry.calls.clone(),
            project,
            uid: registry.uid.clone(),
            installation_id: registry.installation_id.clone(),
        });
        Ok(transport)
    }

    pub fn require_neighbor_general(
        &self,
        caller: Option<&str>,
        nonce: Option<&str>,
    ) -> Result<(), String> {
        let mux = self.mux.read();
        let tab = caller
            .and_then(|id| mux.get_tab(id))
            .ok_or("Only your General Agent may author Neighbor permissions")?;
        if !authoring_allowed(
            crate::service::general_agent::is_general_agent(tab)
                && tab.neighbor_authority.is_none(),
            tab.launch_session_authenticated(),
            tab.session_binding_nonce.as_deref(),
            nonce,
        ) {
            return Err(
                "Only your authenticated General Agent may author Neighbor permissions".into(),
            );
        }
        Ok(())
    }

    pub async fn neighbor_runtime(
        &self,
        command: NeighborRuntimeCommand,
        caller: Option<&str>,
        nonce: Option<&str>,
    ) -> Result<Value, String> {
        if let NeighborRuntimeCommand::FlagMessage {
            friendship_id,
            message_id,
            reason,
            category,
        } = &command
        {
            return self
                .flag_neighbor_message(friendship_id, message_id, reason, *category, caller, nonce)
                .await;
        }
        self.authorize_neighbor_runtime(&command, caller, nonce)
            .await?;
        if let NeighborRuntimeCommand::StopConcierge { friendship_id } = &command {
            return self.stop_neighbor_concierge(friendship_id).await;
        }
        if let NeighborRuntimeCommand::Send { body, .. } = &command {
            codeterm_neighbors::wire::validate_nonempty_text(body)
                .map_err(|error| error.to_string())?;
            codeterm_neighbors::wire::validate_message_size(body)
                .map_err(|error| error.to_string())?;
        }
        if command.is_local_read() {
            return self.neighbor_local_read(&command, caller, nonce).await;
        }
        let result = self
            .neighbor_operation(owner::Operation::Command {
                command,
                caller: caller.map(str::to_owned),
                nonce: nonce.map(str::to_owned),
            })
            .await?;
        match result {
            owner::Outcome::Value(value) => Ok(value),
        }
    }

    async fn neighbor_runtime_owned(
        &self,
        command: NeighborRuntimeCommand,
        caller: Option<&str>,
        nonce: Option<&str>,
        generation: u64,
    ) -> Result<Value, String> {
        self.authorize_neighbor_runtime(&command, caller, nonce)
            .await?;
        if command.is_local_read() {
            return self.neighbor_local_read(&command, caller, nonce).await;
        }
        if !matches!(
            &command,
            NeighborRuntimeCommand::Status
                | NeighborRuntimeCommand::NotificationPreference { .. }
                | NeighborRuntimeCommand::Permission { .. }
                | NeighborRuntimeCommand::OwnKeys
                | NeighborRuntimeCommand::DeliveryStatus { .. }
                | NeighborRuntimeCommand::Revoke
        ) {
            self.require_neighbors_enabled()
                .map_err(|error| error.to_string())?;
        }
        let (transport, registry, scope, metadata) = self.neighbor_ports().await?;
        let registry = match &command {
            NeighborRuntimeCommand::Enroll {
                takeover_confirmed: true,
                expected_receiver_revision,
            } => Arc::new(FirebaseRegistry {
                enrollment: Some(codeterm_neighbors::adapter::Enrollment {
                    installation_only: false,
                    expected_revision: *expected_receiver_revision,
                    takeover: true,
                    operation_id: None,
                }),
                ..registry.as_ref().clone()
            }),
            NeighborRuntimeCommand::Enroll { .. } => {
                return Err("Receive here requires confirmation".into())
            }
            _ => registry,
        };
        let friend_mutation = matches!(&command, NeighborRuntimeCommand::SetFriendReceiver { .. });
        let receiver_status = if friend_mutation {
            self.refresh_neighbor_receiver_readonly(&registry, generation)
                .await?
        } else {
            self.refresh_neighbor_receiver(&registry, &scope, generation)
                .await?
        };
        let lifecycle = InstallationKeys {
            store: &OsKeyStore,
            registry: registry.as_ref(),
            scope: scope.clone(),
            uid: &transport.uid,
        };
        let receiver_mutation = matches!(
            &command,
            NeighborRuntimeCommand::Enroll { .. }
                | NeighborRuntimeCommand::SetFriendReceiver { .. }
                | NeighborRuntimeCommand::Rotate
                | NeighborRuntimeCommand::Revoke
        );
        let result: codeterm_neighbors::Result<Value> = async {
            match command {
                NeighborRuntimeCommand::StopConcierge { .. } => Err(Error::InvalidRequest),
                NeighborRuntimeCommand::PauseFriend { friendship_id } => {
                    self.set_neighbor_paused(
                        &transport,
                        &metadata,
                        &friendship_id,
                        true,
                        generation,
                    )
                    .await
                }
                NeighborRuntimeCommand::ResumeFriend { friendship_id } => {
                    self.set_neighbor_paused(
                        &transport,
                        &metadata,
                        &friendship_id,
                        false,
                        generation,
                    )
                    .await
                }
                NeighborRuntimeCommand::SetNotificationPreference {
                    friendship_id,
                    notify,
                } => {
                    if !hex_id(&friendship_id, 64) {
                        return Err(Error::InvalidEnvelope);
                    }
                    transport.peer_keys(&friendship_id).await?;
                    let mut settings = metadata.settings().await?;
                    settings.notifications.insert(friendship_id.clone(), notify);
                    self.require_neighbor_history_generation(generation)?;
                    metadata.save_settings(settings).await?;
                    self.ui.emit(
                        "neighbor-notification-changed",
                        json!({"friendship_id":friendship_id,"notify":notify}),
                    );
                    Ok(json!({"friendship_id":friendship_id,"notify":notify}))
                }
                NeighborRuntimeCommand::SetDailyLimit {
                    friendship_id,
                    daily_limit,
                } => {
                    if !hex_id(&friendship_id, 64) {
                        return Err(Error::InvalidEnvelope);
                    }
                    let result = transport
                        .calls
                        .invoke(
                            "friendsSetDailyLimit",
                            json!({"friendship_id":friendship_id,"daily_limit":daily_limit}),
                        )
                        .await?;
                    self.ui.emit(
                        "neighbor-daily-limit-changed",
                        json!({"friendship_id":friendship_id,"daily_limit":result["daily_limit"]}),
                    );
                    Ok(result)
                }
                NeighborRuntimeCommand::FlagMessage { .. }
                | NeighborRuntimeCommand::History { .. }
                | NeighborRuntimeCommand::Status
                | NeighborRuntimeCommand::Permission { .. }
                | NeighborRuntimeCommand::NotificationPreference { .. } => {
                    Err(Error::InvalidEnvelope)
                }
                NeighborRuntimeCommand::ChangeAgent {
                    friendship_id,
                    provider,
                    model,
                    reasoning_effort,
                    service_tier,
                } => {
                    if !hex_id(&friendship_id, 64) {
                        return Err(Error::InvalidEnvelope);
                    }
                    let options = super::storage::ConciergeOptions { reasoning_effort, service_tier };
                    self.validate_neighbor_launch_options(&provider, model.as_deref(), &options)
                        .map_err(|_| Error::InvalidRequest)?;
                    transport.peer_keys(&friendship_id).await?;
                    let tab = self
                        .replace_neighbor_concierge(
                            &metadata,
                            &friendship_id,
                            Some(provider),
                            model,
                            options.clone(),
                        )
                        .await
                        .map_err(concierge::launch_error)?;
                    Ok(json!({"gateway_tab":tab,"reasoning_effort":options.reasoning_effort,"service_tier":options.service_tier}))
                }
                NeighborRuntimeCommand::Enroll {
                    takeover_confirmed,
                    expected_receiver_revision,
                } => {
                    if expected_receiver_revision != receiver_status.receiver_revision {
                        return Err(Error::ReceiverRevisionStale);
                    }
                    let keys = lifecycle
                        .enroll_and_enable(takeover_confirmed, &mut rand::rngs::OsRng, &metadata)
                        .await?;
                    Ok(json!({"verifier_key_id":keys.verifier_key_id(&transport.uid)}))
                }
                NeighborRuntimeCommand::SetFriendReceiver { friendship_id,receiver_operation,takeover_confirmed,expected_receiver_revision } => {
                    self.set_neighbor_friend_receiver(&registry,&scope,&metadata,&friendship_id,receiver_operation,
                        takeover_confirmed,expected_receiver_revision,generation).await
                }
                NeighborRuntimeCommand::Retry { friendship_id } => {
                    if !hex_id(&friendship_id, 64) {
                        return Err(Error::InvalidEnvelope);
                    }
                    if let Some(listener) = self
                        .neighbor_cell()
                        .listeners
                        .lock()
                        .active
                        .get(&friendship_id)
                    {
                        listener.retry.notify_one();
                    }
                    Ok(json!({"status":"reconnecting"}))
                }
                NeighborRuntimeCommand::Rotate => {
                    if !receiver_status.receiving_here() {
                        return Err(Error::NotReceiver);
                    }
                    self.stop_neighbor_listeners();
                    let keys = lifecycle.rotate(&mut rand::rngs::OsRng).await?;
                    Ok(json!({"verifier_key_id":keys.verifier_key_id(&transport.uid)}))
                }
                NeighborRuntimeCommand::Revoke => {
                    self.stop_neighbor_listeners();
                    self.retire_local_neighbor_concierges(&metadata,&transport.uid,generation).await.map_err(|_| Error::Gateway)?;
                    if receiver_status.receiver_installation_id.as_deref() == Some(transport.installation_id.as_str()) {
                        if let Err(error) = registry.disable_receiving(receiver_status.receiver_revision).await {
                            log::debug!("[neighbors] account receiver stop refused; revoking this installation's keys: {error}");
                        }
                    }
                    lifecycle.revoke().await?;
                    let mut settings = metadata.settings().await?;
                    settings.enabled = false;
                    metadata.save_settings(settings).await?;
                    Ok(json!({"status":"revoked"}))
                }
                NeighborRuntimeCommand::OwnKeys => {
                    transport.calls.invoke("friendsOwnKeys", json!({})).await
                }
                NeighborRuntimeCommand::Bind {
                    friendship_id,
                    gateway_tab,
                } => {
                    self.bind_neighbor_concierge(
                        &metadata,
                        &transport,
                        &friendship_id,
                        &gateway_tab,
                    )
                    .await
                }
                NeighborRuntimeCommand::SetPermission {
                    friendship_id,
                    permission,
                } => {
                    let current = metadata
                        .settings()
                        .await?
                        .permissions
                        .get(&friendship_id)
                        .cloned()
                        .unwrap_or_default();
                    let permission = permission::merge(current, permission)?;
                    self.set_neighbor_permission(&metadata, &transport, &friendship_id, permission)
                        .await
                }
                NeighborRuntimeCommand::Send {
                    friendship_id,
                    kind,
                    body,
                    in_reply_to,
                } => {
                    self.send_neighbor(
                        &transport,
                        &lifecycle,
                        &metadata,
                        Outgoing {
                            friendship: friendship_id,
                            kind,
                            body: OutboundBody::Granted(Zeroizing::new(body)),
                            in_reply_to,
                            author: self.neighbor_author(caller)?,
                        },
                    )
                    .await
                }
                NeighborRuntimeCommand::DeliveryStatus { message_id } => {
                    if !hex_id(&message_id, 32) {
                        return Err(Error::InvalidEnvelope);
                    }
                    let result = transport.delivery_status(&message_id).await?;
                    if self.require_neighbors_enabled().is_ok() {
                        self.update_neighbor_history_status(
                            &metadata,
                            &message_id,
                            result.into(),
                            generation,
                        )
                        .await?;
                    }
                    Ok(json!(result))
                }
            }
        }
        .await;
        if receiver_mutation {
            self.neighbor_cell().receiver.lock().take();
            self.forget_neighbor_registry();
        }
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                if friend_mutation
                    && matches!(
                        error,
                        Error::ReceiverRevisionStale
                            | Error::ReceiverElsewhere
                            | Error::NotReceiver
                    )
                {
                    self.forget_neighbor_registry();
                    let _ = self
                        .start_neighbor_listeners(
                            transport.clone(),
                            registry.clone(),
                            lifecycle.scope.clone(),
                            metadata.clone(),
                            generation,
                        )
                        .await;
                    return Err(error.to_string());
                }
                if matches!(
                    error,
                    Error::ReceiverRevisionStale
                        | Error::ReceiverElsewhere
                        | Error::Transport
                        | Error::NotReceiver
                ) {
                    let _ = self
                        .refresh_neighbor_receiver(&registry, &lifecycle.scope, generation)
                        .await;
                }
                return Err(error.to_string());
            }
        };
        self.require_neighbor_generation(generation)?;
        let scope = lifecycle.scope.clone();
        drop(lifecycle);
        if self
            .start_neighbor_listeners(transport, registry, scope, metadata, generation)
            .await
            .is_err()
        {
            self.ui.emit(
                "neighbor-runtime-status",
                json!({"status":"stopped","reason":"Neighbor listener unavailable"}),
            );
        }
        Ok(result)
    }

    pub(super) fn stop_neighbor_friendship_listeners(
        &self,
        friendship: &str,
    ) -> Vec<tokio::task::JoinHandle<()>> {
        self.forget_neighbor_registry_friendship(friendship);
        let mut targets = vec![];
        {
            let mut listeners = self.neighbor_cell().listeners.lock();
            if let Some(handle) = listeners.active.remove(friendship) {
                let _ = handle.shutdown.send(true);
                targets.push(handle.task);
            }
            if let Some(task) = listeners.retired.remove(friendship) {
                targets.push(task);
            }
        }
        self.neighbor_cell()
            .pending_counts
            .lock()
            .remove(friendship);
        self.neighbor_cell().states.lock().remove(friendship);
        self.neighbor_cell().failures.lock().remove(friendship);
        self.neighbor_cell().names.lock().remove(friendship);
        targets
    }

    pub(super) fn neighbor_listener_friendships(&self) -> Vec<String> {
        let listeners = self.neighbor_cell().listeners.lock();
        listeners
            .active
            .keys()
            .chain(listeners.retired.keys())
            .cloned()
            .collect()
    }

    pub(crate) fn stop_neighbor_listeners(&self) {
        self.stop_neighbor_listeners_state(
            codeterm_neighbors::supervisor::ListenerState::Stopped,
            None,
        );
    }

    pub(super) fn stop_neighbor_listeners_state(
        &self,
        status: codeterm_neighbors::supervisor::ListenerState,
        error: Option<Value>,
    ) {
        self.neighbor_cell().pending_counts.lock().clear();
        self.neighbor_cell()
            .listener_generation
            .fetch_add(1, Ordering::AcqRel);
        let mut listeners = self.neighbor_cell().listeners.lock();
        let active = std::mem::take(&mut listeners.active);
        for (friendship, listener) in active {
            let _ = listener.shutdown.send(true);
            listeners.retired.insert(friendship.clone(), listener.task);
            self.neighbor_cell()
                .states
                .lock()
                .insert(friendship.clone(), status);
            let mut view = json!({"friendship_id":friendship,"status":status});
            if let Some(error) = &error {
                self.neighbor_cell()
                    .failures
                    .lock()
                    .insert(friendship.clone(), error.clone());
                view["error"] = error.clone();
            }
            self.ui.emit("neighbor-runtime-status", view);
        }
    }

    pub(super) async fn neighbor_receiver_changed(
        &self,
        friendship: Option<&str>,
    ) -> Result<(), String> {
        self.require_neighbors_enabled()
            .map_err(|e| e.to_string())?;
        self.neighbor_cell().receiver.lock().take();
        self.forget_neighbor_registry();
        if let Some(friendship) = friendship {
            self.stop_neighbor_concierge_listener(friendship);
            let version = self.forget_neighbor_friend_receiver(friendship);
            self.ui.emit("neighbor-runtime-status",json!({"friendship_id":friendship,"receiver_unavailable":true,"friend_receivers_version":version}));
        } else {
            let version = self.forget_neighbor_friend_receivers();
            self.ui.emit(
                "neighbor-runtime-status",
                json!({"receiver_unavailable":true,"friend_receivers_version":version}),
            );
        }
        self.neighbor_operation(owner::Operation::ReceiverChanged)
            .await?;
        Ok(())
    }

    pub(super) fn pause_neighbor_reception(&self) {
        self.forget_neighbor_friend_receivers();
        self.neighbor_cell().watch_states.lock().clear();
        self.neighbor_cell().control_cancel.notify_waiters();
        self.cancel_neighbor_restoration();
        self.forget_neighbor_registry();
        self.neighbor_cell().receiver.lock().take();
        if let Some(task) = self.neighbor_cell().control.lock().take() {
            task.abort();
        }
        self.stop_neighbor_listeners();
        self.neighbor_cell()
            .bootstrapped
            .store(false, Ordering::Release);
    }

    pub(crate) fn neighbor_identity_changed(&self) {
        self.forget_neighbor_friend_receivers();
        self.neighbor_cell().control_cancel.notify_waiters();
        self.cancel_neighbor_restoration();
        self.forget_neighbor_registry();
        self.neighbor_cell().names.lock().clear();
        self.neighbor_cell().pauses.clear();
        self.neighbor_cell().handling.clear();
        self.neighbor_cell().restore_grants.lock().clear();
        if let Some(task) = self.neighbor_cell().control.lock().take() {
            task.abort();
        }
        self.neighbor_cell().receiver.lock().take();
        self.stop_neighbor_updates();
        self.neighbor_cell().watch_states.lock().clear();
        self.neighbor_cell()
            .generation
            .fetch_add(1, Ordering::AcqRel);
        self.stop_neighbor_listeners();
        self.neighbor_cell()
            .bootstrapped
            .store(false, std::sync::atomic::Ordering::Release);
    }
}

fn now() -> Timestamp {
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    Timestamp {
        seconds: time.as_secs(),
        nanoseconds: time.subsec_nanos(),
    }
}
