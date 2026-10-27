mod authority;
pub(super) mod callable;
mod concierge;
mod gateway;
mod listener;
mod local;
pub(crate) mod logout;
mod owner;
mod pending;
mod receiver;
mod restoration;
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
    pub(super) concierge_accounts: parking_lot::Mutex<HashMap<String, String>>,
    registry_snapshot: parking_lot::Mutex<Option<Value>>,
    owner: parking_lot::Mutex<Option<tokio::sync::mpsc::Sender<owner::Request>>>,
    pub(super) generation: AtomicU64,
    listener_generation: AtomicU64,
    listeners: parking_lot::Mutex<listener::Slots>,
    states: parking_lot::Mutex<HashMap<String, codeterm_neighbors::supervisor::ListenerState>>,
    failures: parking_lot::Mutex<HashMap<String, Value>>,
    pending_counts: parking_lot::Mutex<HashMap<String, u64>>,
    names: parking_lot::Mutex<HashMap<String, String>>,
    receiver: parking_lot::Mutex<Option<codeterm_neighbors::adapter::ReceiverStatus>>,
    control: parking_lot::Mutex<Option<tokio::task::JoinHandle<()>>>,
    control_start: tokio::sync::Mutex<()>,
    control_cancel: tokio::sync::Notify,
    bootstrapped: std::sync::atomic::AtomicBool,
    restoration: parking_lot::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum NeighborRuntimeCommand {
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
    },
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
    author: super::history::Author,
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
        self.authorize_neighbor_runtime(&command, caller, nonce)
            .await?;
        if let NeighborRuntimeCommand::Send { body, .. } = &command {
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
        let receiver_status = self
            .refresh_neighbor_receiver(&registry, &scope, generation)
            .await?;
        let lifecycle = InstallationKeys {
            store: &OsKeyStore,
            registry: registry.as_ref(),
            scope,
            uid: &transport.uid,
        };
        let receiver_mutation = matches!(
            &command,
            NeighborRuntimeCommand::Enroll { .. }
                | NeighborRuntimeCommand::Rotate
                | NeighborRuntimeCommand::Revoke
        );
        let result: codeterm_neighbors::Result<Value> = async {
            match command {
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
                NeighborRuntimeCommand::History { .. }
                | NeighborRuntimeCommand::Status
                | NeighborRuntimeCommand::Permission { .. }
                | NeighborRuntimeCommand::NotificationPreference { .. } => {
                    Err(Error::InvalidEnvelope)
                }
                NeighborRuntimeCommand::ChangeAgent {
                    friendship_id,
                    provider,
                    model,
                } => {
                    if !hex_id(&friendship_id, 64) {
                        return Err(Error::InvalidEnvelope);
                    }
                    transport.peer_keys(&friendship_id).await?;
                    let tab = self
                        .replace_neighbor_concierge(
                            &metadata,
                            &friendship_id,
                            Some(provider),
                            model,
                        )
                        .await
                        .map_err(concierge::launch_error)?;
                    Ok(json!({"gateway_tab":tab}))
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
                    registry
                        .disable_receiving(receiver_status.receiver_revision)
                        .await?;
                    lifecycle.forget().await?;
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
                    if !hex_id(&friendship_id, 64) {
                        return Err(Error::InvalidEnvelope);
                    }
                    self.validate_neighbor_gateway(&gateway_tab)
                        .map_err(|_| Error::Gateway)?;
                    transport.peer_keys(&friendship_id).await?;
                    let mut settings = metadata.settings().await?;
                    settings
                        .bindings
                        .insert(friendship_id.clone(), gateway_tab.clone());
                    metadata.save_settings(settings).await?;
                    self.stop_neighbor_listeners();
                    self.emit_state_changed(crate::service::MuxEmitCause::TabMetadata);
                    self.ui.emit(
                        "neighbor-agent-changed",
                        json!({"friendship_id":friendship_id,"gateway_tab":gateway_tab}),
                    );
                    Ok(json!({"status":"bound"}))
                }
                NeighborRuntimeCommand::SetPermission {
                    friendship_id,
                    permission,
                } => {
                    if !hex_id(&friendship_id, 64) {
                        return Err(Error::InvalidEnvelope);
                    }
                    permission.validate()?;
                    transport.peer_keys(&friendship_id).await?;
                    self.set_neighbor_permission(
                        &metadata,
                        &transport.uid,
                        &friendship_id,
                        permission,
                    )
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
        }
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

    async fn send_neighbor(
        &self,
        transport: &FirebaseTransport,
        lifecycle: &InstallationKeys<'_>,
        metadata: &Metadata,
        outgoing: Outgoing,
    ) -> codeterm_neighbors::Result<Value> {
        let generation = self.neighbor_history_generation();
        let friendship = outgoing.friendship.clone();
        let agent = super::notifications::AgentNotice::for_author(
            &outgoing.author,
            outgoing.in_reply_to.is_some(),
        );
        let result = self
            .send_neighbor_once(transport, lifecycle, metadata, outgoing)
            .await;
        let rejection = match &result {
            Ok(result) => result.rejection,
            Err(error) => super::delivery::Rejection::from_error(*error),
        };
        if let (Some(agent), Some(rejection)) = (agent, rejection) {
            if let Err(error) = self
                .notify_neighbor_rejection(metadata, &friendship, agent, rejection, generation)
                .await
            {
                log::debug!("[neighbors] rejected-send notification unavailable: {error}");
            }
        }
        result.map(|result| result.receipt)
    }

    async fn send_neighbor_once(
        &self,
        transport: &FirebaseTransport,
        lifecycle: &InstallationKeys<'_>,
        metadata: &Metadata,
        outgoing: Outgoing,
    ) -> codeterm_neighbors::Result<super::delivery::SendResult> {
        self.require_neighbors_enabled()?;
        let generation = self.neighbor_history_generation();
        let Outgoing {
            friendship,
            kind,
            body,
            in_reply_to,
            author,
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
        let refusal = kind == Kind::Refusal && in_reply_to.is_some();
        if refusal
            && !metadata
                .has_incoming(&friendship, in_reply_to.as_deref().ok_or(Error::Gateway)?)
                .await?
        {
            return Err(Error::Gateway);
        }
        let body = match body {
            OutboundBody::Granted(body) if refusal && body.len() <= 256 => body,
            OutboundBody::Granted(body) if kind != Kind::Refusal => {
                permission.require(kind)?;
                body
            }
            OutboundBody::Refusal if refusal => {
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
            friendship_id: friendship.clone(),
            message_id: uuid::Uuid::new_v4().simple().to_string(),
            sender_uid: transport.uid.clone(),
            recipient_uid: peers.peer_uid,
            kind,
            in_reply_to: in_reply_to.clone(),
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
        self.require_neighbor_history_generation(generation)?;
        self.append_neighbor_history(
            metadata,
            &friendship,
            super::history::Entry {
                direction: "out".into(),
                author,
                body: body.to_string(),
                sender_uid: transport.uid.clone(),
                at: now().seconds * 1000 + u64::from(now().nanoseconds / 1_000_000),
                message_id: signed.envelope.message_id.clone(),
                kind,
                in_reply_to,
                outcome: None,
                reason: None,
                limit: None,
                resets_at: None,
                retry_after_seconds: None,
                submission: None,
                delivery_status: Some(codeterm_neighbors::wire::DeliveryStatus::Pending),
            },
            generation,
        )
        .await?;
        self.require_neighbor_history_generation(generation)?;
        let result = transport.send(&signed).await;
        let status = match &result {
            Ok(status) => *status,
            Err(Error::Transport) => codeterm_neighbors::wire::DeliveryStatus::Pending,
            Err(_) => codeterm_neighbors::wire::DeliveryStatus::NotDelivered,
        };
        if self.require_neighbor_history_generation(generation).is_ok() {
            let refusal = result
                .as_ref()
                .err()
                .copied()
                .and_then(super::delivery::Rejection::from_error);
            if let Err(error) = self
                .update_neighbor_history_status(
                    metadata,
                    &signed.envelope.message_id,
                    super::history::HistoryReceipt {
                        delivery: status.into(),
                        rejection: refusal,
                        submission: Some(match &result {
                            Ok(_) => super::history::Submission::Accepted,
                            Err(Error::Transport) => super::history::Submission::Unconfirmed,
                            Err(_) => super::history::Submission::Refused,
                        }),
                    },
                    generation,
                )
                .await
            {
                log::debug!("[neighbors] local delivery status write failed: {error}");
            }
        }
        let refusal = result
            .as_ref()
            .err()
            .copied()
            .and_then(super::delivery::Rejection::from_error);
        if let Some(rejection) = refusal {
            return Ok(super::delivery::SendResult {
                receipt: rejection.receipt(&signed.envelope.message_id),
                rejection: Some(rejection),
            });
        }
        let receipt = match result {
            Ok(status) => json!({"message_id":signed.envelope.message_id,"status":status}),
            Err(Error::Transport) => {
                json!({"message_id":signed.envelope.message_id,"status":"pending","submission":"unconfirmed"})
            }
            Err(error) => return Err(error),
        };
        Ok(super::delivery::SendResult {
            receipt,
            rejection: None,
        })
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
        self.neighbor_cell().pending_counts.lock().clear();
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

    async fn start_neighbor_listeners(
        &self,
        transport: Arc<FirebaseTransport>,
        registry: Arc<FirebaseRegistry>,
        scope: KeyScope,
        metadata: Metadata,
        generation: u64,
    ) -> Result<(), String> {
        self.start_neighbor_listeners_mode(transport, registry, scope, metadata, generation, true)
            .await
    }

    async fn start_neighbor_listeners_mode(
        &self,
        transport: Arc<FirebaseTransport>,
        registry: Arc<FirebaseRegistry>,
        scope: KeyScope,
        metadata: Metadata,
        generation: u64,
        recover_keys: bool,
    ) -> Result<(), String> {
        self.require_neighbors_enabled()
            .map_err(|error| error.to_string())?;
        self.watch_neighbor_receiver(transport.clone(), generation)
            .await?;
        let status = if recover_keys {
            self.refresh_neighbor_receiver(&registry, &scope, generation)
                .await?
        } else {
            self.refresh_neighbor_receiver_readonly(&registry, generation)
                .await?
        };
        if !status.receiving_here() {
            self.stop_neighbor_listeners();
            return Ok(());
        }
        let settings = metadata.settings().await.map_err(|e| e.to_string())?;
        self.remember_neighbor_concierge_accounts(&transport.uid, &settings);
        let keys = InstallationKeys {
            store: &OsKeyStore,
            registry: registry.as_ref(),
            scope: scope.clone(),
            uid: &transport.uid,
        };
        let recovered = if recover_keys {
            keys.reconcile().await
        } else {
            keys.load().await
        }
        .map_err(|e| e.to_string())?;
        if recovered.is_none() {
            return if recover_keys {
                Ok(())
            } else {
                Err(Error::LocalKeyUnavailable.to_string())
            };
        }
        if !recover_keys
            && recovered.as_ref().is_some_and(|keys| {
                status.receiver_key_id.as_deref()
                    != Some(keys.verifier_key_id(&transport.uid).as_str())
            })
        {
            return Err(Error::LocalKeyUnavailable.to_string());
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
        let cached_friends = self.neighbor_cell().registry_snapshot.lock().clone();
        let friends = match cached_friends {
            Some(friends) => friends,
            None => {
                self.neighbor_registry(super::NeighborCommand::List {})
                    .await?
            }
        };
        self.remember_neighbor_names(&friends);
        if friends.get("access").and_then(Value::as_str) != Some("paid") {
            self.stop_neighbor_listeners();
            return Ok(());
        }
        self.require_neighbor_generation(generation)?;
        let listener_generation = self
            .neighbor_cell()
            .listener_generation
            .load(Ordering::Acquire);
        let friendships = friends["friends"]
            .as_array()
            .ok_or("Invalid Neighbors response")?;
        for friend in friendships {
            let friendship = friend["friendship_id"]
                .as_str()
                .ok_or("Invalid Neighbors response")?
                .to_owned();
            if !hex_id(&friendship, 64) {
                continue;
            }
            let tab = settings
                .bindings
                .get(&friendship)
                .filter(|tab| self.validate_neighbor_gateway(tab).is_ok())
                .cloned();
            let mut listeners = self.neighbor_cell().listeners.lock();
            if listeners
                .active
                .get(&friendship)
                .is_some_and(|task| !task.task.is_finished() && task.gateway_tab == tab)
            {
                continue;
            }
            if let Some(previous) = listeners.active.remove(&friendship) {
                let _ = previous.shutdown.send(true);
                listeners.retired.insert(friendship.clone(), previous.task);
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
                    tab: tab.clone(),
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
                    gateway_tab: tab,
                    task,
                    retry,
                    shutdown,
                },
            );
        }
        Ok(())
    }

    pub(crate) fn neighbor_identity_changed(&self) {
        self.neighbor_cell().control_cancel.notify_waiters();
        self.cancel_neighbor_restoration();
        self.forget_neighbor_registry();
        self.neighbor_cell().names.lock().clear();
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
