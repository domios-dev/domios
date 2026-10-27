pub(super) mod callable;
mod gateway;
mod listener;
pub(crate) mod logout;
mod owner;
mod receiver;

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
    owner: parking_lot::Mutex<Option<tokio::sync::mpsc::Sender<owner::Request>>>,
    generation: AtomicU64,
    listener_generation: AtomicU64,
    listeners: parking_lot::Mutex<listener::Slots>,
    states: parking_lot::Mutex<HashMap<String, codeterm_neighbors::supervisor::ListenerState>>,
    receiver: parking_lot::Mutex<Option<codeterm_neighbors::adapter::ReceiverStatus>>,
    control: parking_lot::Mutex<Option<tokio::task::JoinHandle<()>>>,
    bootstrapped: std::sync::atomic::AtomicBool,
}

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum NeighborRuntimeCommand {
    Status,
    Enroll {
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
        permission: Permission,
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
}

enum OutboundBody {
    Granted(Zeroizing<String>),
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
    fn neighbor_cell(&self) -> &NeighborRuntimeCell {
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
            .fresh_session()
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
        Ok((transport, registry, scope, metadata))
    }

    fn require_neighbor_general(
        &self,
        caller: Option<&str>,
        nonce: Option<&str>,
    ) -> Result<(), String> {
        let mux = self.mux.read();
        let tab = caller
            .and_then(|id| mux.get_tab(id))
            .ok_or("Only your General Agent may author Neighbor permissions")?;
        if !authoring_allowed(
            crate::service::general_agent::is_general_agent(tab),
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
        if !matches!(
            &command,
            NeighborRuntimeCommand::Status
                | NeighborRuntimeCommand::Permission { .. }
                | NeighborRuntimeCommand::OwnKeys
                | NeighborRuntimeCommand::DeliveryStatus { .. }
                | NeighborRuntimeCommand::Revoke
        ) {
            self.require_neighbors_enabled()
                .map_err(|error| error.to_string())?;
        }
        if matches!(&command, NeighborRuntimeCommand::SetPermission { .. })
            || (caller.is_some() && matches!(&command, NeighborRuntimeCommand::Bind { .. }))
        {
            self.require_neighbor_general(caller, nonce)?;
        }
        let (transport, registry, scope, metadata) = self.neighbor_ports().await?;
        let registry = match &command {
            NeighborRuntimeCommand::Enroll {
                takeover_confirmed: true,
                expected_receiver_revision,
            } => Arc::new(FirebaseRegistry {
                enrollment: Some(codeterm_neighbors::adapter::Enrollment {
                    expected_revision: *expected_receiver_revision,
                    takeover: true,
                }),
                ..registry.as_ref().clone()
            }),
            NeighborRuntimeCommand::Enroll { .. } => {
                return Err("Receive here requires confirmation".into())
            }
            _ => registry,
        };
        let receiver_status = self
            .refresh_neighbor_receiver(&registry, generation)
            .await?;
        let lifecycle = InstallationKeys {
            store: &OsKeyStore,
            registry: registry.as_ref(),
            scope,
            uid: &transport.uid,
        };
        let result: codeterm_neighbors::Result<Value> = async {
            match command {
                NeighborRuntimeCommand::Status => {
                    let settings = metadata.settings().await?;
                    let keys = if receiver_status.receiving_here() { lifecycle.reconcile().await? } else { None };
                    let id = keys.map(|keys| keys.verifier_key_id(&transport.uid));
                    let states = self.neighbor_cell().states.lock().clone();
                    Ok(json!({"enabled":self.configuration().neighbors.enabled,"enrolled":id.is_some(),"verifier_key_id":id,"bindings":settings.bindings,"listeners":states,"receiving_here":receiver_status.receiving_here(),"receiver_machine_label":receiver_status.receiver_machine_label,"receiving_enabled":receiver_status.receiving_enabled,"revision":receiver_status.receiver_revision}))
                }
                NeighborRuntimeCommand::Enroll { takeover_confirmed, expected_receiver_revision } => {
                    if expected_receiver_revision != receiver_status.receiver_revision { return Err(Error::ReceiverRevisionStale); }
                    let keys = lifecycle.enroll_and_enable(takeover_confirmed, &mut rand::rngs::OsRng, &metadata).await?;
                    Ok(json!({"verifier_key_id":keys.verifier_key_id(&transport.uid)}))
                }
                NeighborRuntimeCommand::Retry { friendship_id } => {
                    if !hex_id(&friendship_id, 64) { return Err(Error::InvalidEnvelope); }
                    if let Some(listener) = self.neighbor_cell().listeners.lock().active.get(&friendship_id) { listener.retry.notify_one(); }
                    Ok(json!({"status":"reconnecting"}))
                }
                NeighborRuntimeCommand::Rotate => {
                    if !receiver_status.receiving_here() { return Err(Error::NotReceiver); }
                    self.stop_neighbor_listeners();
                    let keys = lifecycle.rotate(&mut rand::rngs::OsRng).await?;
                    Ok(json!({"verifier_key_id":keys.verifier_key_id(&transport.uid)}))
                }
                NeighborRuntimeCommand::Revoke => {
                    self.stop_neighbor_listeners();
                    registry.disable_receiving(receiver_status.receiver_revision).await?;
                    lifecycle.forget().await?;
                    let mut settings = metadata.settings().await?;
                    settings.enabled = false;
                    metadata.save_settings(settings).await?;
                    Ok(json!({"status":"revoked"}))
                }
                NeighborRuntimeCommand::OwnKeys => transport.calls.invoke("friendsOwnKeys", json!({})).await,
                NeighborRuntimeCommand::Bind { friendship_id, gateway_tab } => {
                    if !hex_id(&friendship_id, 64) { return Err(Error::InvalidEnvelope); }
                    self.validate_neighbor_gateway(&gateway_tab).map_err(|_| Error::Gateway)?;
                    transport.peer_keys(&friendship_id).await?;
                    let mut settings = metadata.settings().await?;
                    settings.bindings.insert(friendship_id, gateway_tab);
                    metadata.save_settings(settings).await?;
                    self.stop_neighbor_listeners();
                    Ok(json!({"status":"bound"}))
                }
                NeighborRuntimeCommand::Permission { friendship_id } => {
                    if !hex_id(&friendship_id, 64) { return Err(Error::InvalidEnvelope); }
                    Ok(json!(metadata.settings().await?.permissions.get(&friendship_id).cloned().unwrap_or_default()))
                }
                NeighborRuntimeCommand::SetPermission { friendship_id, permission } => {
                    if !hex_id(&friendship_id, 64) { return Err(Error::InvalidEnvelope); }
                    permission.validate()?;
                    transport.peer_keys(&friendship_id).await?;
                    let mut settings = metadata.settings().await?;
                    settings.permissions.insert(friendship_id, permission);
                    self.stop_neighbor_listeners();
                    metadata.save_settings(settings).await?;
                    Ok(json!({"status":"saved"}))
                }
                NeighborRuntimeCommand::Send { friendship_id, kind, body, in_reply_to } => {
                    self.send_neighbor(&transport, &lifecycle, &metadata, Outgoing { friendship: friendship_id, kind, body: OutboundBody::Granted(Zeroizing::new(body)), in_reply_to }).await
                }
                NeighborRuntimeCommand::DeliveryStatus { message_id } => {
                    if !hex_id(&message_id, 32) { return Err(Error::InvalidEnvelope); }
                    Ok(json!(transport.delivery_status(&message_id).await?))
                }
            }
        }.await;
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                if matches!(
                    error,
                    Error::ReceiverRevisionStale
                        | Error::ReceiverElsewhere
                        | Error::Transport
                        | Error::NotReceiver
                ) {
                    let _ = self.refresh_neighbor_receiver(&registry, generation).await;
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

    async fn send_neighbor(
        &self,
        transport: &FirebaseTransport,
        lifecycle: &InstallationKeys<'_>,
        metadata: &Metadata,
        outgoing: Outgoing,
    ) -> codeterm_neighbors::Result<Value> {
        self.require_neighbors_enabled()?;
        let Outgoing {
            friendship,
            kind,
            body,
            in_reply_to,
        } = outgoing;
        if !hex_id(&friendship, 64) {
            return Err(Error::InvalidEnvelope);
        }
        let permission = metadata
            .settings()
            .await?
            .permissions
            .get(&friendship)
            .cloned()
            .unwrap_or_default();
        let body = match body {
            OutboundBody::Granted(body) if permission.allows(kind) => body,
            OutboundBody::Refusal if kind == Kind::Answer && in_reply_to.is_some() => {
                Zeroizing::new(codeterm_neighbors::gateway::REFUSAL_BODY.to_owned())
            }
            _ => return Err(Error::Gateway),
        };
        let keys = lifecycle.load().await?.ok_or(Error::Revoked)?;
        if !lifecycle
            .registry
            .active_key_ids()
            .await?
            .contains(&keys.verifier_key_id(&transport.uid))
        {
            return Err(Error::Revoked);
        }
        let peers = transport.peer_keys(&friendship).await?;
        let peer = peers
            .keys
            .iter()
            .max_by_key(|key| key.created_at)
            .ok_or(Error::InvalidKey)?;
        let mut envelope = Envelope {
            schema: SCHEMA.into(),
            friendship_id: friendship,
            message_id: uuid::Uuid::new_v4().simple().to_string(),
            sender_uid: transport.uid.clone(),
            recipient_uid: peers.peer_uid,
            kind,
            in_reply_to,
            ciphertext: String::new(),
            nonce: String::new(),
            enc_key_ids: EncryptionKeyIds {
                sender: String::new(),
                recipient: String::new(),
            },
            scope_sha256: permission.scope_sha256()?,
            verdict: "pass".into(),
            verifier_key_id: keys.verifier_key_id(&transport.uid),
            issued_at: now().seconds,
        };
        keys.encrypt(
            &mut envelope,
            &peer.encryption_public_key_spki_b64,
            &body,
            &mut rand::rngs::OsRng,
        )?;
        let signed = keys.sign(envelope)?;
        self.require_neighbors_enabled()?;
        match transport.send(&signed).await {
            Ok(status) => Ok(json!({"message_id":signed.envelope.message_id,"status":status})),
            Err(Error::Transport) => Ok(
                json!({"message_id":signed.envelope.message_id,"status":"pending","submission":"unconfirmed"}),
            ),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn stop_neighbor_listeners(&self) {
        self.neighbor_cell()
            .listener_generation
            .fetch_add(1, Ordering::AcqRel);
        let mut listeners = self.neighbor_cell().listeners.lock();
        let active = std::mem::take(&mut listeners.active);
        for (friendship, listener) in active {
            let _ = listener.shutdown.send(true);
            listeners.retired.insert(friendship.clone(), listener.task);
            self.neighbor_cell().states.lock().insert(
                friendship.clone(),
                codeterm_neighbors::supervisor::ListenerState::Stopped,
            );
            self.ui.emit(
                "neighbor-runtime-status",
                json!({"friendship_id":friendship,"status":"stopped"}),
            );
        }
    }

    pub(super) async fn neighbor_receiver_changed(&self) -> Result<(), String> {
        self.require_neighbors_enabled()
            .map_err(|e| e.to_string())?;
        self.neighbor_cell().receiver.lock().take();
        self.stop_neighbor_listeners();
        self.neighbor_operation(owner::Operation::ReceiverChanged)
            .await?;
        Ok(())
    }

    pub(super) fn pause_neighbor_reception(&self) {
        if let Some(task) = self.neighbor_cell().control.lock().take() {
            task.abort();
        }
        self.stop_neighbor_listeners();
        self.neighbor_cell()
            .bootstrapped
            .store(false, Ordering::Release);
    }

    async fn start_neighbor_listeners(
        &self,
        transport: Arc<FirebaseTransport>,
        registry: Arc<FirebaseRegistry>,
        scope: KeyScope,
        metadata: Metadata,
        generation: u64,
    ) -> Result<(), String> {
        self.require_neighbors_enabled()
            .map_err(|error| error.to_string())?;
        self.watch_neighbor_receiver(transport.clone(), generation)
            .await?;
        let status = self
            .refresh_neighbor_receiver(&registry, generation)
            .await?;
        if !status.receiving_here() {
            self.stop_neighbor_listeners();
            return Ok(());
        }
        let settings = metadata.settings().await.map_err(|e| e.to_string())?;
        let recovered = (InstallationKeys {
            store: &OsKeyStore,
            registry: registry.as_ref(),
            scope: scope.clone(),
            uid: &transport.uid,
        })
        .reconcile()
        .await
        .map_err(|e| e.to_string())?;
        if recovered.is_none() {
            return Ok(());
        }
        drop(recovered);
        if !settings.enabled {
            let mut recovered_settings = settings.clone();
            recovered_settings.enabled = true;
            metadata
                .save_settings(recovered_settings)
                .await
                .map_err(|e| e.to_string())?;
        }
        if self
            .neighbor_registry(super::NeighborCommand::List {})
            .await?
            .get("access")
            .and_then(Value::as_str)
            != Some("paid")
        {
            self.stop_neighbor_listeners();
            return Ok(());
        }
        self.require_neighbor_generation(generation)?;
        let listener_generation = self
            .neighbor_cell()
            .listener_generation
            .load(Ordering::Acquire);
        for (friendship, tab) in settings.bindings {
            if !hex_id(&friendship, 64) || self.validate_neighbor_gateway(&tab).is_err() {
                continue;
            }
            let mut listeners = self.neighbor_cell().listeners.lock();
            if listeners
                .active
                .get(&friendship)
                .is_some_and(|task| !task.task.is_finished())
            {
                continue;
            }
            let service = self.clone();
            let transport = transport.clone();
            let metadata = metadata.clone();
            let scope = scope.clone();
            let id = friendship.clone();
            let retry = Arc::new(tokio::sync::Notify::new());
            let (shutdown, stopped) = tokio::sync::watch::channel(false);
            let previous = listeners.retired.remove(&friendship);
            let task = tokio::spawn(listener::run(
                listener::Session {
                    service,
                    transport,
                    scope,
                    metadata,
                    friendship,
                    tab,
                    generation: listener_generation,
                    receiver_revision: status.receiver_revision,
                    retry: retry.clone(),
                    registry: registry.clone(),
                    stopped,
                },
                previous,
            ));
            listeners.active.insert(
                id,
                listener::Handle {
                    task,
                    retry,
                    shutdown,
                },
            );
        }
        Ok(())
    }

    #[cfg(not(any(test, feature = "test-support")))]
    pub(crate) fn restore_neighbors_once(&self) {
        if self.require_neighbors_enabled().is_err() {
            return;
        }
        if self
            .neighbor_cell()
            .bootstrapped
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            return;
        }
        let service = self.clone();
        tokio::spawn(async move {
            let _ = service.neighbor_operation(owner::Operation::Restore).await;
        });
    }

    pub(crate) fn neighbor_identity_changed(&self) {
        if let Some(task) = self.neighbor_cell().control.lock().take() {
            task.abort();
        }
        self.neighbor_cell().receiver.lock().take();
        self.stop_neighbor_updates();
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
