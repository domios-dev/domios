use super::lifecycle::AccountMesh;
use super::operations::Signed;
use super::state::{AccountMeshRefusal, Publication};
use super::wire::{MeshEndpoint, MeshOperation, MeshResult};

impl AccountMesh {
    pub(super) async fn publish(
        &self,
        signed: &Signed,
        endpoint: Option<MeshEndpoint>,
    ) -> Result<bool, AccountMeshRefusal> {
        let expected_metadata_revision = self.metadata_revision(signed).await?;
        let operation = match endpoint {
            Some(endpoint) => MeshOperation::Publish {
                expected_metadata_revision,
                endpoint: endpoint.wire(),
            },
            None => MeshOperation::Withdraw {
                expected_metadata_revision,
            },
        };
        let result = self.admission(signed, operation).await;
        if result == Err(AccountMeshRefusal::Conflict) {
            self.with(|inner| inner.metadata_revision = None);
        }
        let installation = self.installation();
        let (published, metadata_revision) = match result? {
            MeshResult::Published(result) if result.installation_id == installation => {
                self.with(|inner| {
                    if inner.is_current(signed.epoch) {
                        inner.published_until = result.endpoint.expires_at;
                    }
                });
                (true, result.metadata_revision)
            }
            MeshResult::Withdrawn(result) if result.installation_id == installation => {
                (false, result.metadata_revision)
            }
            _ => return Err(AccountMeshRefusal::Invalid),
        };
        self.remember_metadata(signed.epoch, metadata_revision, None);
        Ok(published)
    }

    pub(super) async fn reconcile_publication(
        &self,
        signed: &Signed,
    ) -> Result<(), AccountMeshRefusal> {
        let result = self.reconcile_publication_once(signed).await;
        if let Err(refusal) = result {
            self.with(|inner| {
                if inner.is_current(signed.epoch) {
                    inner.update(&self.watch, |state| {
                        state.publication_failure = Some(refusal)
                    });
                }
            });
            self.push().await;
        }
        result
    }

    async fn reconcile_publication_once(&self, signed: &Signed) -> Result<(), AccountMeshRefusal> {
        let ready = self.ports.endpoint.ready_endpoint();
        let membership = self
            .state()
            .current_membership(self.now())
            .ok_or(AccountMeshRefusal::Unauthenticated)?
            .token
            .clone();
        let changed = self.with(|inner| {
            inner.gossiped_endpoint.as_ref() != Some(&(ready.clone(), membership.clone()))
        });
        if changed {
            let sequence = self.with(|inner| {
                if !inner.is_current(signed.epoch) {
                    return Err(AccountMeshRefusal::Cancelled);
                }
                let sequence = inner
                    .record
                    .endpoint_sequence
                    .checked_add(1)
                    .ok_or(AccountMeshRefusal::Unavailable)?;
                inner.record.endpoint_sequence = sequence;
                Ok(sequence)
            })?;
            self.persist_checked()
                .await
                .map_err(|_| AccountMeshRefusal::Unavailable)?;
            self.push().await;
            let installed = self
                .ports
                .transport
                .endpoint_changed(ready.clone(), sequence, &membership)
                .await;
            if installed {
                self.with(|inner| {
                    if !inner.is_current(signed.epoch) {
                        return Err(AccountMeshRefusal::Cancelled);
                    }
                    inner.gossiped_endpoint = Some((ready.clone(), membership));
                    Ok(())
                })?;
            } else if ready.is_some() {
                return Err(AccountMeshRefusal::Unavailable);
            }
        }
        let now = self.now();
        let cold_start = ready.is_some()
            && self.with(|inner| {
                inner.published_endpoint != ready
                    || (inner.launch_publication_pending && inner.published_until <= now)
            })
            && !self.ports.transport.peers_reachable();
        if cold_start {
            self.with(|inner| inner.record.published = true);
            self.persist_checked()
                .await
                .map_err(|_| AccountMeshRefusal::Unavailable)?;
            // Reachability can change while durable publication intent is written.
            if !self.ports.transport.peers_reachable() {
                self.publish(signed, ready.clone()).await?;
                self.with(|inner| {
                    if inner.is_current(signed.epoch) {
                        inner.published_endpoint = ready.clone();
                    }
                });
            }
        }
        self.with(|inner| {
            if !inner.is_current(signed.epoch) {
                return Err(AccountMeshRefusal::Cancelled);
            }
            inner.launch_publication_pending &= ready.is_none();
            inner.update(&self.watch, |state| {
                state.publication = if ready.is_some() {
                    Publication::Published
                } else {
                    Publication::NotReady
                };
                state.publication_failure = None;
            });
            Ok(())
        })?;
        self.persist().await;
        self.push().await;
        Ok(())
    }
}
