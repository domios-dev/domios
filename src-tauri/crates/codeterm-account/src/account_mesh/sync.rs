use super::lifecycle::AccountMesh;
use super::operations::Signed;
use super::state::{AccountMeshRefusal, Cleanup, Publication, RegistryObservation};
use super::wire::{MeshOperation, MeshResult, PublicationState, RegistryPresence};

impl AccountMesh {
    pub(super) async fn sync_registry(&self, signed: &Signed) -> Result<(), AccountMeshRefusal> {
        let ready = self.ports.endpoint.ready_endpoint();
        let intended = self.with(|inner| {
            if !inner.is_current(signed.epoch) {
                return false;
            }
            if ready.is_some() {
                inner.record.published = true;
            }
            inner.sync_pending = true;
            true
        });
        if !intended {
            return Err(AccountMeshRefusal::Cancelled);
        }
        self.persist().await;
        let operation = MeshOperation::Sync {
            presence: RegistryPresence::Present,
            endpoint: ready.map(|endpoint| endpoint.wire()),
            default_display_name: self
                .config
                .default_display_name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_owned),
        };
        let result = self.admission(signed, operation).await;
        let result = match result {
            Ok(MeshResult::Sync(result))
                if result.installation_id == self.installation()
                    && result.presence == RegistryPresence::Present =>
            {
                result
            }
            Ok(_) => return Err(AccountMeshRefusal::Invalid),
            Err(refusal) => {
                self.with(|inner| {
                    if inner.is_current(signed.epoch) {
                        inner.update(&self.watch, |state| {
                            state.publication_failure = Some(refusal)
                        });
                    }
                });
                return Err(refusal);
            }
        };
        let endpoint = result.endpoint.as_ref().and_then(|endpoint| {
            super::wire::MeshEndpoint::parse(&endpoint.tailnet_ip, endpoint.api_port)
                .map(|parsed| parsed.with_transport(endpoint.transport.clone()))
        });
        let expires_at = result
            .endpoint
            .as_ref()
            .map_or(0, |endpoint| endpoint.expires_at);
        let published = result.publication == PublicationState::Complete
            && endpoint.is_some()
            && expires_at > self.now();
        let cleanup_confirmed = published
            || (result.publication == PublicationState::None && result.endpoint.is_none());
        self.with(|inner| {
            if !inner.is_current(signed.epoch) {
                return Err(AccountMeshRefusal::Cancelled);
            }
            inner.metadata_revision = Some(result.metadata_revision);
            inner.registry_presence = Some(result.presence);
            inner.sync_pending = false;
            inner.published_endpoint = endpoint;
            inner.published_until = expires_at;
            inner.record.published = result.publication != PublicationState::None;
            let cleanup_confirmed = cleanup_confirmed && inner.record.cleanup_pending;
            if cleanup_confirmed {
                inner.record.cleanup_pending = false;
            }
            inner.update(&self.watch, |state| {
                if cleanup_confirmed {
                    state.cleanup = Cleanup::Confirmed;
                }
                if let Some(own) = state.self_installation.as_mut() {
                    own.metadata_revision = result.metadata_revision;
                    own.display_name = result.display_name;
                }
                state.publication = if published {
                    Publication::Published
                } else {
                    Publication::NotReady
                };
                state.publication_failure = None;
                state.registry = Some(RegistryObservation {
                    observed_at: self.now(),
                    registry_revision: result.registry.registry_revision,
                    rows: result.registry.rows,
                    cursor: result.registry.cursor,
                });
                state.registry_failure = None;
            });
            Ok(())
        })?;
        self.persist().await;
        self.push().await;
        self.ports
            .transport
            .presence_changed(result.metadata_revision)
            .await;
        Ok(())
    }
}
