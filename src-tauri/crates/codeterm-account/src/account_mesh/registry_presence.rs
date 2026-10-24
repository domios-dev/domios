use super::lifecycle::AccountMesh;
use super::operations::Signed;
use super::state::AccountMeshRefusal;
use super::wire::{MeshOperation, MeshResult, RegistryPresence};
use crate::Session;

impl AccountMesh {
    pub(super) async fn set_registry_presence(
        &self,
        signed: &Signed,
        presence: RegistryPresence,
    ) -> Result<(), AccountMeshRefusal> {
        if signed.epoch != 0
            && self.with(|inner| {
                inner.is_current(signed.epoch) && inner.registry_presence == Some(presence)
            })
        {
            return Ok(());
        }
        let expected_metadata_revision = self.metadata_revision(signed).await?;
        let result = self
            .admission(
                signed,
                MeshOperation::Presence {
                    expected_metadata_revision,
                    presence,
                },
            )
            .await;
        if matches!(
            result,
            Err(AccountMeshRefusal::Conflict | AccountMeshRefusal::Forbidden)
        ) {
            self.with(|inner| inner.metadata_revision = None);
        }
        let MeshResult::Presence(result) = result? else {
            return Err(AccountMeshRefusal::Invalid);
        };
        if result.installation_id != self.installation() || result.presence != presence {
            return Err(AccountMeshRefusal::Invalid);
        }
        if signed.epoch != 0 && !self.with(|inner| inner.is_current(signed.epoch)) {
            return Err(AccountMeshRefusal::Cancelled);
        }
        self.remember_metadata(signed.epoch, result.metadata_revision, None);
        self.with(|inner| {
            if inner.is_current(signed.epoch) {
                inner.registry_presence = Some(presence);
            }
        });
        self.ports
            .transport
            .presence_changed(result.metadata_revision)
            .await;
        Ok(())
    }

    pub(super) async fn announce_signed_out(
        &self,
        session: &Session,
    ) -> Result<(), AccountMeshRefusal> {
        let signed = self.outgoing_signed(session).await?;
        self.set_registry_presence(&signed, RegistryPresence::SignedOut)
            .await
    }
}
