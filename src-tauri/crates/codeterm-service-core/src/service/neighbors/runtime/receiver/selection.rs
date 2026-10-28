use super::*;

#[derive(Clone, Copy, Debug, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Selection { Set, Reset }

#[async_trait::async_trait]
trait SelectionPort: Send + Sync {
    async fn account_receiver(&self) -> codeterm_neighbors::Result<ReceiverStatus>;
    async fn enroll(&self, enrollment: codeterm_neighbors::adapter::Enrollment) -> codeterm_neighbors::Result<()>;
    async fn select(&self, command: Value) -> codeterm_neighbors::Result<Value>;
}

struct RelaySelection<'a> {
    service: &'a ServiceContext,
    registry: &'a FirebaseRegistry,
    scope: &'a KeyScope,
    generation: u64,
}

#[async_trait::async_trait]
impl SelectionPort for RelaySelection<'_> {
    async fn account_receiver(&self) -> codeterm_neighbors::Result<ReceiverStatus> {
        self.registry.receiver_status().await
    }

    async fn enroll(&self, enrollment: codeterm_neighbors::adapter::Enrollment) -> codeterm_neighbors::Result<()> {
        self.service.require_neighbor_generation(self.generation).map_err(|_| Error::Revoked)?;
        let registry = FirebaseRegistry { enrollment:Some(enrollment),..self.registry.clone() };
        InstallationKeys { store:&OsKeyStore,registry:&registry,scope:self.scope.clone(),uid:&registry.uid }
            .enroll(true,&mut rand::rngs::OsRng).await?;
        Ok(())
    }

    async fn select(&self, command: Value) -> codeterm_neighbors::Result<Value> {
        self.service.require_neighbor_generation(self.generation).map_err(|_| Error::Revoked)?;
        self.registry.calls.invoke("friendsSetFriendReceiver",command).await
    }
}

async fn select_friend_receiver(
    port: &impl SelectionPort, installation: &str, friendship: &str, selection: Selection, expected_revision: u64,
) -> codeterm_neighbors::Result<Value> {
    if matches!(selection,Selection::Set) {
        let account = port.account_receiver().await?;
        port.enroll(codeterm_neighbors::adapter::Enrollment {
            expected_revision:account.receiver_revision,takeover:false,operation_id:None,
            installation_only:account.receiver_installation_id.as_deref() != Some(installation),
        }).await?;
    }
    port.select(json!({
        "installation_id":installation,"friendship_id":friendship,"operation":selection,
        "takeover":true,"expected_revision":expected_revision,"operation_id":uuid::Uuid::new_v4().simple().to_string(),
    })).await
}

impl ServiceContext {
    pub(in crate::service::neighbors::runtime) async fn set_neighbor_friend_receiver(
        &self, registry: &FirebaseRegistry, scope: &KeyScope, metadata: &Metadata,
        friendship: &str, selection: Selection, confirmed: bool, expected_revision: u64, generation: u64,
    ) -> codeterm_neighbors::Result<Value> {
        if !hex_id(friendship,64) || expected_revision > codeterm_neighbors::wire::MAX_SAFE_INTEGER {
            return Err(Error::InvalidRequest);
        }
        if !confirmed { return Err(Error::Gateway); }
        self.require_neighbor_generation(generation).map_err(|_| Error::Revoked)?;
        let port = RelaySelection { service:self,registry,scope,generation };
        let result = select_friend_receiver(&port,&registry.installation_id,friendship,selection,expected_revision).await?;
        self.require_neighbor_generation(generation).map_err(|_| Error::Revoked)?;
        if result["schema_version"] != 1 || result["friendship_id"] != friendship { return Err(Error::InvalidEnvelope); }
        let projection: codeterm_neighbors::wire::EffectiveReceiver = serde_json::from_value(result["receiver"].clone()).map_err(|_| Error::InvalidEnvelope)?;
        let receiver = super::friends::FriendReceiver::from_projection(projection,&registry.installation_id).map_err(|_| Error::InvalidEnvelope)?;
        self.stop_neighbor_concierge_listener(friendship);
        self.forget_neighbor_registry();
        if receiver.status.receiving_here() {
            let local = InstallationKeys { store:&OsKeyStore,registry,scope:scope.clone(),uid:&registry.uid }.load().await?;
            if local.as_ref().map(|key| key.verifier_key_id(&registry.uid)).as_deref() != receiver.status.receiver_key_id.as_deref() || local.is_none() {
                let version = self.forget_neighbor_friend_receiver(friendship);
                self.ui.emit("neighbor-runtime-status",json!({"friendship_id":friendship,"receiver_unavailable":true,"friend_receivers_version":version}));
                return Err(Error::LocalKeyUnavailable);
            }
        }
        let changes = self.remember_neighbor_friend_receiver(friendship,receiver,generation).map_err(|_| Error::Revoked)?;
        self.publish_neighbor_friend_receiver_changes(metadata,&registry.uid,changes,generation).await.map_err(|_| Error::Gateway)?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
