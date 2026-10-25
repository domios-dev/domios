mod callable;
mod gateway;

use super::storage::Metadata;
use crate::service::ServiceContext;
use codeterm_neighbors::{
    adapter::FirebaseTransport,
    crypto::PrivateKeys,
    keys::{InstallationKeys, KeyRegistry, KeyScope, OsKeyStore},
    permissions::Permission,
    receive::{CursorStore, NeighborTransport, Receiver},
    wire::{hex_id, EncryptionKeyIds, Envelope, Kind, Timestamp, SCHEMA},
    Error,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
use zeroize::Zeroizing;

#[derive(Default)]
pub struct NeighborRuntimeCell {
    operations: Mutex<()>,
    listeners: parking_lot::Mutex<HashMap<String, tokio::task::JoinHandle<()>>>,
    bootstrapped: std::sync::atomic::AtomicBool,
}

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum NeighborRuntimeCommand {
    Status,
    Enroll {
        takeover_confirmed: bool,
    },
    Rotate,
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
    body: Zeroizing<String>,
    in_reply_to: Option<String>,
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

    async fn neighbor_ports(&self) -> Result<(Arc<FirebaseTransport>, KeyScope, Metadata), String> {
        let account = self.account().await?;
        let session = account
            .fresh_session()
            .await
            .map_err(|_| "Sign in to use Neighbors")?
            .ok_or("Sign in to use Neighbors")?;
        let uid = session.identity.uid.clone();
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
        let calls = Arc::new(callable::Calls {
            service: self.clone(),
            uid: uid.clone(),
            generation: session.login_generation,
        });
        let transport = Arc::new(FirebaseTransport {
            calls,
            project,
            uid: uid.clone(),
        });
        let scope = KeyScope::installation(self.installation_id().await?.as_str(), &uid)
            .map_err(|e| e.to_string())?;
        let metadata = Metadata::new(self.account_data_root()?, &uid);
        Ok((transport, scope, metadata))
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
        let _operation = self.neighbor_cell().operations.lock().await;
        let (transport, scope, metadata) = self.neighbor_ports().await?;
        let lifecycle = InstallationKeys {
            store: &OsKeyStore,
            registry: transport.as_ref(),
            scope,
            uid: &transport.uid,
        };
        let result: codeterm_neighbors::Result<Value> = async {
            match command {
                NeighborRuntimeCommand::Status => {
                    let settings = metadata.settings().await?;
                    if !settings.enabled { return Ok(json!({"enrolled":false,"verifier_key_id":null,"bindings":settings.bindings})); }
                    let keys = lifecycle.load().await?;
                    let id = keys.map(|keys| keys.verifier_key_id(&transport.uid));
                    let active = transport.active_key_ids().await?;
                    Ok(json!({"enrolled":id.as_ref().is_some_and(|id| active.contains(id)),"verifier_key_id":id,"bindings":metadata.settings().await?.bindings}))
                }
                NeighborRuntimeCommand::Enroll { takeover_confirmed } => {
                    let keys = lifecycle.enroll(takeover_confirmed, &mut rand::rngs::OsRng).await?;
                    let mut settings = metadata.settings().await?;
                    settings.enabled = true;
                    metadata.save_settings(settings).await?;
                    Ok(json!({"verifier_key_id":keys.verifier_key_id(&transport.uid)}))
                }
                NeighborRuntimeCommand::Rotate => {
                    self.stop_neighbor_listeners();
                    let keys = lifecycle.rotate(&mut rand::rngs::OsRng).await?;
                    Ok(json!({"verifier_key_id":keys.verifier_key_id(&transport.uid)}))
                }
                NeighborRuntimeCommand::Revoke => {
                    self.stop_neighbor_listeners();
                    lifecycle.revoke().await?;
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
                    self.require_neighbor_general(caller, nonce).map_err(|_| Error::Gateway)?;
                    if !hex_id(&friendship_id, 64) { return Err(Error::InvalidEnvelope); }
                    permission.validate()?;
                    transport.peer_keys(&friendship_id).await?;
                    let mut settings = metadata.settings().await?;
                    settings.permissions.insert(friendship_id, permission);
                    metadata.save_settings(settings).await?;
                    Ok(json!({"status":"saved"}))
                }
                NeighborRuntimeCommand::Send { friendship_id, kind, body, in_reply_to } => {
                    self.send_neighbor(&transport, &lifecycle, &metadata, Outgoing { friendship: friendship_id, kind, body: Zeroizing::new(body), in_reply_to }).await
                }
                NeighborRuntimeCommand::DeliveryStatus { message_id } => {
                    if !hex_id(&message_id, 32) { return Err(Error::InvalidEnvelope); }
                    Ok(json!({"status":transport.delivery_status(&message_id).await?}))
                }
            }
        }.await;
        let result = result.map_err(|e| e.to_string())?;
        let scope = lifecycle.scope.clone();
        drop(lifecycle);
        if self
            .start_neighbor_listeners(transport, scope, metadata)
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
        if !permission.allows(kind) {
            return Err(Error::Gateway);
        }
        let keys = lifecycle.load().await?.ok_or(Error::Revoked)?;
        if !transport
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
        match transport.send(&signed).await {
            Ok(status) => Ok(json!({"message_id":signed.envelope.message_id,"status":status})),
            Err(Error::Transport) => Ok(
                json!({"message_id":signed.envelope.message_id,"status":"pending","submission":"unconfirmed"}),
            ),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn stop_neighbor_listeners(&self) {
        for (_, listener) in self.neighbor_cell().listeners.lock().drain() {
            listener.abort();
        }
    }

    async fn start_neighbor_listeners(
        &self,
        transport: Arc<FirebaseTransport>,
        scope: KeyScope,
        metadata: Metadata,
    ) -> Result<(), String> {
        let settings = metadata.settings().await.map_err(|e| e.to_string())?;
        if !settings.enabled {
            return Ok(());
        }
        if self
            .neighbor_registry(super::NeighborCommand::List)
            .await?
            .get("access")
            .and_then(Value::as_str)
            != Some("paid")
        {
            self.stop_neighbor_listeners();
            return Ok(());
        }
        let active = transport
            .active_key_ids()
            .await
            .map_err(|e| e.to_string())?;
        let Some(keys) = (InstallationKeys {
            store: &OsKeyStore,
            registry: transport.as_ref(),
            scope: scope.clone(),
            uid: &transport.uid,
        })
        .load()
        .await
        .map_err(|e| e.to_string())?
        else {
            return Ok(());
        };
        if !active.contains(&keys.verifier_key_id(&transport.uid)) {
            return Ok(());
        }
        drop(keys);
        for (friendship, tab) in settings.bindings {
            if !hex_id(&friendship, 64) || self.validate_neighbor_gateway(&tab).is_err() {
                continue;
            }
            let mut listeners = self.neighbor_cell().listeners.lock();
            if listeners
                .get(&friendship)
                .is_some_and(|task| !task.is_finished())
            {
                continue;
            }
            let service = self.clone();
            let transport = transport.clone();
            let metadata = metadata.clone();
            let scope = scope.clone();
            let id = friendship.clone();
            let reported_id = id.clone();
            let task = tokio::spawn(async move {
                let result = service
                    .listen_neighbor(&transport, scope, metadata, friendship, tab)
                    .await;
                if let Err(error) = result {
                    service.ui.emit("neighbor-runtime-status", json!({"friendship_id":reported_id,"status":"stopped","reason":error.to_string()}));
                }
            });
            listeners.insert(id, task);
        }
        Ok(())
    }

    async fn listen_neighbor(
        &self,
        transport: &FirebaseTransport,
        scope: KeyScope,
        metadata: Metadata,
        friendship: String,
        tab: String,
    ) -> codeterm_neighbors::Result<()> {
        let lifecycle = InstallationKeys {
            store: &OsKeyStore,
            registry: transport,
            scope,
            uid: &transport.uid,
        };
        let cursor = metadata.load(&friendship).await?;
        let mut listener = transport
            .listen_from_cursor(&friendship, cursor.as_ref())
            .await?;
        let gateway = gateway::Gateway {
            service: self.clone(),
            metadata: metadata.clone(),
            friendship: friendship.clone(),
            tab,
        };
        while let Some(mut messages) = listener.next().await? {
            let _operation = self.neighbor_cell().operations.lock().await;
            if self
                .neighbor_registry(super::NeighborCommand::List)
                .await
                .map_err(|_| Error::Transport)?
                .get("access")
                .and_then(Value::as_str)
                != Some("paid")
            {
                return Err(Error::Locked);
            }
            let keys: PrivateKeys = lifecycle.load().await?.ok_or(Error::Revoked)?;
            if !transport
                .active_key_ids()
                .await?
                .contains(&keys.verifier_key_id(&transport.uid))
            {
                return Err(Error::Revoked);
            }
            let mut receiver = Receiver {
                transport,
                cursors: &metadata,
                gateway: &gateway,
                private_keys: &keys,
                recipient_uid: &transport.uid,
                friendship_id: &friendship,
            };
            receiver.catch_up(&mut messages, now()).await?;
        }
        Err(Error::Transport)
    }

    #[cfg(not(any(test, feature = "test-support")))]
    pub(crate) fn restore_neighbors_once(&self) {
        if self
            .neighbor_cell()
            .bootstrapped
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            return;
        }
        let service = self.clone();
        tokio::spawn(async move {
            let _operation = service.neighbor_cell().operations.lock().await;
            if let Ok((transport, scope, metadata)) = service.neighbor_ports().await {
                let _ = service
                    .start_neighbor_listeners(transport, scope, metadata)
                    .await;
            }
        });
    }

    pub(crate) fn neighbor_identity_changed(&self) {
        self.stop_neighbor_listeners();
        self.neighbor_cell()
            .bootstrapped
            .store(false, std::sync::atomic::Ordering::Release);
    }

    pub(crate) async fn revoke_neighbors_for_logout(&self) -> Result<(), String> {
        let _operation = self.neighbor_cell().operations.lock().await;
        self.neighbor_identity_changed();
        let Some(session) = self.account().await?.session().await else {
            return Ok(());
        };
        let metadata = Metadata::new(self.account_data_root()?, &session.identity.uid);
        let mut settings = metadata.settings().await.map_err(|e| e.to_string())?;
        if !settings.enabled {
            return Ok(());
        }
        let (transport, scope, _) = self.neighbor_ports().await?;
        let keys = InstallationKeys {
            store: &OsKeyStore,
            registry: transport.as_ref(),
            scope,
            uid: &transport.uid,
        };
        keys.revoke().await.map_err(|e| e.to_string())?;
        settings.enabled = false;
        metadata
            .save_settings(settings)
            .await
            .map_err(|e| e.to_string())
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
