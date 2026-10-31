use super::*;

#[derive(Clone, Copy, Debug, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Selection {
    Set,
    Reset,
}

pub(in crate::service::neighbors::runtime) struct Committed {
    pub result: Value,
    pub friendship: String,
    pub revision: u64,
    pub generation: u64,
    pub login_generation: u64,
    pub registry: Arc<FirebaseRegistry>,
    pub scope: KeyScope,
    pub metadata: Metadata,
}

#[async_trait::async_trait]
trait SelectionPort: Send + Sync {
    async fn enroll(&self) -> codeterm_neighbors::Result<()>;
    async fn select(&self, command: Value) -> codeterm_neighbors::Result<Value>;
}

#[derive(Default)]
struct Registration(tokio::sync::Mutex<Option<RegistrationBinding>>);

#[derive(Clone, PartialEq, Eq)]
struct RegistrationBinding {
    generation: u64,
    login_generation: u64,
    uid: String,
    installation_id: String,
    machine_label: String,
    key_id: String,
}

impl RegistrationBinding {
    fn new(
        registry: &FirebaseRegistry,
        generation: u64,
        login_generation: u64,
        key_id: String,
    ) -> Self {
        Self {
            generation,
            login_generation,
            key_id,
            uid: registry.uid.clone(),
            installation_id: registry.installation_id.clone(),
            machine_label: registry.machine_label.clone(),
        }
    }
}

async fn refresh_registration(
    registered: &mut Option<RegistrationBinding>,
    binding: RegistrationBinding,
    registry: &impl codeterm_neighbors::keys::KeyRegistry,
    keys: &codeterm_neighbors::crypto::PrivateKeys,
) -> codeterm_neighbors::Result<()> {
    if registered.as_ref() != Some(&binding) {
        let enrolled = registry
            .enroll(&keys.signing_spki(), &keys.encryption_spki())
            .await?;
        if enrolled != binding.key_id {
            return Err(Error::InvalidKey);
        }
        *registered = Some(binding);
    }
    Ok(())
}

struct RelaySelection<'a> {
    service: &'a ServiceContext,
    registry: &'a FirebaseRegistry,
    scope: &'a KeyScope,
    generation: u64,
}

#[async_trait::async_trait]
impl SelectionPort for RelaySelection<'_> {
    async fn enroll(&self) -> codeterm_neighbors::Result<()> {
        self.service
            .require_neighbor_generation(self.generation)
            .map_err(|_| Error::Revoked)?;
        let registry = self.registry;
        let account = self.service.account().await.map_err(|_| Error::Transport)?;
        let session = account.session().await.ok_or(Error::SessionRevoked)?;
        if session.identity.uid != registry.uid {
            return Err(Error::SessionRevoked);
        }
        let registration = self.service.extensions.get_or_init(Registration::default);
        let mut registered = registration.0.lock().await;
        let store = self.service.neighbor_key_store()?;
        let lifecycle = InstallationKeys {
            store: &store,
            registry,
            scope: self.scope.clone(),
            uid: &registry.uid,
        };
        let existing = lifecycle.reconcile().await?;
        let keys = match existing {
            Some(keys) => {
                let id = keys.verifier_key_id(&registry.uid);
                refresh_registration(
                    &mut registered,
                    RegistrationBinding::new(
                        registry,
                        self.generation,
                        session.login_generation,
                        id,
                    ),
                    registry,
                    &keys,
                )
                .await?;
                keys
            }
            None => lifecycle.enroll(&mut rand::rngs::OsRng).await?,
        };
        self.service
            .require_neighbor_generation(self.generation)
            .map_err(|_| Error::Revoked)?;
        *registered = Some(RegistrationBinding::new(
            registry,
            self.generation,
            session.login_generation,
            keys.verifier_key_id(&registry.uid),
        ));
        Ok(())
    }

    async fn select(&self, command: Value) -> codeterm_neighbors::Result<Value> {
        self.service
            .require_neighbor_generation(self.generation)
            .map_err(|_| Error::Revoked)?;
        self.registry
            .calls
            .invoke("friendsSetFriendReceiver", command)
            .await
    }
}

fn validate_selection(
    status: &ReceiverStatus,
    revision: u64,
    confirmed: bool,
) -> codeterm_neighbors::Result<()> {
    if revision != status.receiver_revision {
        return Err(Error::ReceiverRevisionStale);
    }
    if status.receiver_installation_id.is_some() && !status.is_me && !confirmed {
        return Err(Error::ReceiverElsewhere);
    }
    Ok(())
}

async fn select_friend_receiver(
    port: &impl SelectionPort,
    installation: &str,
    friendship: &str,
    selection: Selection,
    expected_revision: u64,
    takeover: bool,
) -> codeterm_neighbors::Result<Value> {
    if matches!(selection, Selection::Set) {
        port.enroll().await?;
    }
    let command = json!({
        "installation_id":installation,"friendship_id":friendship,"operation":selection,
        "takeover":takeover,"expected_revision":expected_revision,"operation_id":uuid::Uuid::new_v4().simple().to_string(),
    });
    port.select(command).await
}

impl ServiceContext {
    pub(in crate::service::neighbors::runtime) async fn set_neighbor_friend_receiver(
        &self,
        registry: &FirebaseRegistry,
        scope: &KeyScope,
        metadata: &Metadata,
        friendship: &str,
        selection: Selection,
        confirmed: bool,
        expected_revision: u64,
        generation: u64,
    ) -> codeterm_neighbors::Result<Committed> {
        if !hex_id(friendship, 64) || expected_revision > codeterm_neighbors::wire::MAX_SAFE_INTEGER
        {
            return Err(Error::InvalidRequest);
        }
        let account = self.account().await.map_err(|_| Error::Transport)?;
        let session = account.session().await.ok_or(Error::SessionRevoked)?;
        if session.identity.uid != registry.uid {
            return Err(Error::SessionRevoked);
        }
        let list = registry.calls.invoke("friendsList", json!({})).await?;
        let friend = super::super::first_action::friend_status(
            &list,
            friendship,
            &registry.installation_id,
        )?;
        validate_selection(&friend, expected_revision, confirmed)?;
        self.require_neighbor_generation(generation)
            .map_err(|_| Error::Revoked)?;
        let port = RelaySelection {
            service: self,
            registry,
            scope,
            generation,
        };
        let result = select_friend_receiver(
            &port,
            &registry.installation_id,
            friendship,
            selection,
            expected_revision,
            confirmed,
        )
        .await?;
        self.require_neighbor_generation(generation)
            .map_err(|_| Error::Revoked)?;
        let row: codeterm_neighbors::adapter::FriendReceiverRow =
            serde_json::from_value(result["receiver"].clone())
                .map_err(|_| Error::InvalidEnvelope)?;
        if result["schema_version"] != 1
            || row.friendship_id != friendship
            || row.revision < expected_revision
            || row.revision > codeterm_neighbors::wire::MAX_SAFE_INTEGER
        {
            return Err(Error::InvalidEnvelope);
        }
        if !super::super::super::same_session(account.session().await.as_ref(), &session) {
            return Err(Error::SessionRevoked);
        }
        self.require_neighbor_generation(generation).map_err(|_| Error::Revoked)?;
        self.rearm_neighbor_updates();
        self.forget_neighbor_registry();
        Ok(Committed {
            result, friendship: friendship.into(), revision: row.revision, generation,
            login_generation: session.login_generation, registry: Arc::new(registry.clone()),
            scope: scope.clone(), metadata: metadata.clone(),
        })
    }

    pub(in crate::service::neighbors::runtime) async fn reconcile_committed_neighbor_receiver(
        &self,
        committed: &Committed,
        resume: bool,
    ) -> codeterm_neighbors::Result<super::super::super::projection::ReceiverConfirmation> {
        use super::super::super::projection::ReceiverConfirmation;
        self.require_neighbor_generation(committed.generation).map_err(|_| Error::Revoked)?;
        self.require_neighbors_enabled()?;
        let account = self.account().await.map_err(|_| Error::Transport)?;
        let session = account.session().await.ok_or(Error::SessionRevoked)?;
        if session.identity.uid != committed.registry.uid || session.login_generation != committed.login_generation {
            return Err(Error::SessionRevoked);
        }
        self.neighbor_reconciliation().require_readable()?;
        let list = committed.registry.calls.invoke("friendsList", json!({})).await?;
        let status = super::super::first_action::friend_status(&list, &committed.friendship, &committed.registry.installation_id)?;
        if status.receiver_revision < committed.revision
            || resume && (status.receiver_revision != committed.revision || !status.receiving_here())
        {
            return Ok(ReceiverConfirmation::Pending);
        }
        self.require_neighbor_generation(committed.generation).map_err(|_| Error::Revoked)?;
        if !super::super::super::same_session(account.session().await.as_ref(), &session) {
            return Err(Error::SessionRevoked);
        }
        let transport = self.neighbor_transport(&committed.registry).map_err(|_| Error::Transport)?;
        self.start_neighbor_listeners(transport, committed.registry.clone(), committed.scope.clone(), committed.metadata.clone(), committed.generation)
            .await.map_err(|error| {
                log::debug!("[neighbors] committed receiver activation unavailable: {error}");
                Error::Gateway
            })?;
        Ok(ReceiverConfirmation::Observed)
    }
}

#[cfg(test)]
mod tests;
