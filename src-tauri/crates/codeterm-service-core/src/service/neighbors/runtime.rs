mod authority;
pub(super) mod callable;
mod concierge;
pub(super) mod files;
mod first_action;
mod gateway;
mod key_store;
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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::{collections::HashMap, sync::Arc};
use zeroize::Zeroizing;

#[derive(Default)]
struct IdentityPending(AtomicBool);

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
    control_cancel: tokio::sync::Notify,
    pub(super) watch_states: parking_lot::Mutex<std::collections::BTreeMap<String, Value>>,
    bootstrapped: std::sync::atomic::AtomicBool,
    restoration: parking_lot::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

#[cfg(test)]
impl NeighborRuntimeCell {
    pub(in crate::service::neighbors) fn registry_snapshot_for_test(&self) -> Option<Value> {
        self.registry_snapshot.lock().clone()
    }

    pub(in crate::service::neighbors) fn listener_count_for_test(&self) -> usize {
        self.listeners.lock().active.len()
    }
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
    authority: authority::SendAuthority,
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
    pub(crate) async fn neighbor_memory_peer_uid(
        &self,
        authority: &crate::multiplexer::types::NeighborAuthority,
    ) -> Result<String, String> {
        let unavailable =
            "neighbor_memory_author_unavailable: Verified friendship author is unavailable";
        #[cfg(any(test, feature = "test-support"))]
        if let Some(peers) = self.try_ext::<crate::mem_ops::author::FixtureNeighborAuthors>() {
            return peers
                .0
                .read()
                .get(&(authority.uid.clone(), authority.friendship_id.clone()))
                .filter(|uid| !uid.is_empty())
                .cloned()
                .ok_or_else(|| unavailable.into());
        }
        let (registry, _, _) = self.neighbor_key_ports().await.map_err(|_| unavailable)?;
        if registry.uid != authority.uid {
            return Err(unavailable.into());
        }
        let transport = self
            .neighbor_transport(&registry)
            .map_err(|_| unavailable)?;
        let peers = transport
            .peer_keys(&authority.friendship_id)
            .await
            .map_err(|_| unavailable)?;
        if peers.friendship_id != authority.friendship_id || peers.peer_uid.trim().is_empty() {
            return Err(unavailable.into());
        }
        Ok(peers.peer_uid)
    }

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
            .map_err(|_| Error::Transport.to_string())?
            .ok_or_else(|| Error::Revoked.to_string())?;
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
        let send_authority = self.neighbor_send_authority(&command, caller, nonce)?;
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
        let mutation_friend = match &command {
            NeighborRuntimeCommand::SetFriendReceiver { friendship_id, .. } => {
                Some(friendship_id.clone())
            }
            _ => None,
        };
        let friend_mutation = mutation_friend.is_some();
        match &command {
            NeighborRuntimeCommand::SetPermission { friendship_id, .. }
            | NeighborRuntimeCommand::Bind { friendship_id, .. }
            | NeighborRuntimeCommand::ChangeAgent { friendship_id, .. } => {
                self.prepare_neighbor_owner_action(
                    &registry,
                    &scope,
                    &metadata,
                    friendship_id,
                    generation,
                )
                .await?;
            }
            NeighborRuntimeCommand::SetFriendReceiver {
                friendship_id,
                takeover_confirmed,
                expected_receiver_revision,
                ..
            } => {
                self.require_neighbor_receiver_confirmation(
                    &registry,
                    friendship_id,
                    *expected_receiver_revision,
                    *takeover_confirmed,
                )
                .await?;
            }
            _ => {}
        }
        let lifecycle = InstallationKeys {
            store: &self.neighbor_key_store().map_err(|e| e.to_string())?,
            registry: registry.as_ref(),
            scope: scope.clone(),
            uid: &transport.uid,
        };
        let receiver_mutation = matches!(
            &command,
            NeighborRuntimeCommand::SetFriendReceiver { .. }
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
                    self.stop_neighbor_listeners();
                    let keys = lifecycle.rotate(&mut rand::rngs::OsRng).await?;
                    Ok(json!({"verifier_key_id":keys.verifier_key_id(&transport.uid)}))
                }
                NeighborRuntimeCommand::Revoke => {
                    self.stop_neighbor_listeners();
                    self.retire_local_neighbor_concierges(&metadata,&transport.uid,generation).await.map_err(|_| Error::Gateway)?;
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
                            authority: send_authority,
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
            self.rearm_neighbor_updates();
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
                    if error == Error::ReceiverElsewhere {
                        if let Some(friendship) = mutation_friend.as_deref() {
                            if let Some(status) = self.neighbor_friend_receiver(friendship) {
                                let name = self
                                    .neighbor_cell_names()
                                    .get(friendship)
                                    .cloned()
                                    .unwrap_or_else(|| "This friend".into());
                                return Err(first_action::receiver_refusal(
                                    friendship, &name, &status,
                                ));
                            }
                        }
                    }
                    return Err(error.to_string());
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

    pub(super) fn pause_neighbor_reception(&self) {
        if self.require_neighbors_enabled().is_err() {
            self.neighbor_reconciliation().close();
        }
        self.forget_neighbor_friend_receivers();
        self.neighbor_cell().watch_states.lock().clear();
        self.neighbor_cell().control_cancel.notify_waiters();
        self.cancel_neighbor_restoration();
        self.forget_neighbor_registry();
        self.stop_neighbor_listeners();
        self.neighbor_cell()
            .bootstrapped
            .store(false, Ordering::Release);
    }

    pub(crate) fn neighbor_identity_changed(&self) {
        self.extensions
            .get_or_init(IdentityPending::default)
            .0
            .store(true, Ordering::Release);
        self.neighbor_reconciliation().close();
        self.forget_neighbor_friend_receivers();
        self.forget_neighbor_registry();
        self.neighbor_cell().names.lock().clear();
        self.neighbor_cell().pauses.clear();
        self.neighbor_cell().handling.clear();
        self.neighbor_cell().restore_grants.lock().clear();
        self.neighbor_cell().control_cancel.notify_waiters();
        self.cancel_neighbor_restoration();
        self.stop_neighbor_updates();
        self.neighbor_cell().watch_states.lock().clear();
        self.neighbor_cell()
            .generation
            .fetch_add(1, Ordering::AcqRel);
        self.stop_neighbor_listeners();
        self.neighbor_cell()
            .bootstrapped
            .store(false, std::sync::atomic::Ordering::Release);
        let service = self.clone();
        match self.neighbor_reconciliation().submit(
            super::reconciliation::Kind::Retirement,
            move || async move {
                service.finish_neighbor_identity_change();
                Ok(())
            },
        ) {
            Ok(receipt) => {
                crate::async_runtime::spawn(async move {
                    if let Err(error) = receipt.wait().await {
                        log::debug!("[neighbors] identity reconciliation drain failed: {error}");
                    }
                });
            }
            Err(error) => log::debug!("[neighbors] identity reconciliation drain pending: {error}"),
        }
    }

    pub(super) fn finish_neighbor_identity_change(&self) {
        if !self
            .extensions
            .get_or_init(IdentityPending::default)
            .0
            .swap(false, Ordering::AcqRel)
        {
            return;
        }
        self.reset_neighbor_projection_owned();
        self.neighbor_cell().names.lock().clear();
        self.neighbor_cell().pauses.clear();
        self.neighbor_cell().handling.clear();
        self.neighbor_cell().restore_grants.lock().clear();
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
