use super::lifecycle::AccountMesh;
use super::operations::request_id;
use super::state::AccountMeshRefusal;
use super::wire::*;

type Outcome<T> = Result<T, AccountMeshRefusal>;

impl AccountMesh {
    /// Batch-removes up to `BATCH_REMOVE_MAX_ITEMS` stale installations in one backend call.
    pub async fn remove_stale(&self, installation_ids: Vec<String>) -> Outcome<BatchRemoveResult> {
        let own = self.installation();
        let mut unique = Vec::with_capacity(installation_ids.len());
        let mut seen = std::collections::HashSet::with_capacity(installation_ids.len());
        for id in installation_ids {
            if id == own {
                return Err(AccountMeshRefusal::Forbidden);
            }
            crate::InstallationId::parse(id.clone()).map_err(|_| AccountMeshRefusal::Invalid)?;
            if seen.insert(id.clone()) {
                unique.push(id);
            }
        }
        if unique.is_empty() || unique.len() > BATCH_REMOVE_MAX_ITEMS {
            return Err(AccountMeshRefusal::Invalid);
        }
        let mut signed = self.signed(None).await?;
        let _ = self.generation(&signed).await?;
        let request = BatchRemoveRequest {
            schema_version: MESH_SCHEMA_VERSION,
            installation_id: own,
            request_id: request_id(),
            installation_ids: unique,
        };
        let first = self
            .call(
                self.ports
                    .registry
                    .remove_stale_installations(&signed.session, &request),
            )
            .await;
        let result = if first == Err(AccountMeshRefusal::Unauthenticated) {
            let refreshed = self
                .call_session_refresh()
                .await?
                .ok_or(AccountMeshRefusal::Unauthenticated)?;
            if refreshed.identity.uid != signed.session.identity.uid
                || refreshed.login_generation != signed.session.login_generation
                || !self.with(|inner| inner.is_current(signed.epoch))
            {
                return Err(AccountMeshRefusal::Cancelled);
            }
            signed.session = refreshed;
            self.call(
                self.ports
                    .registry
                    .remove_stale_installations(&signed.session, &request),
            )
            .await?
        } else {
            first?
        };
        if result.schema_version != MESH_SCHEMA_VERSION {
            return Err(AccountMeshRefusal::Invalid);
        }
        if let Some(snapshot) = result.signed_registry.as_ref() {
            self.accept_server_registry(snapshot, &signed.account, signed.epoch)
                .await?;
        }
        let removed_ids: Vec<String> = result
            .results
            .iter()
            .filter(|row| {
                matches!(
                    row.status,
                    BatchRemoveRowStatus::Removed | BatchRemoveRowStatus::AlreadyRemoved
                )
            })
            .map(|row| row.installation_id.clone())
            .collect();
        if !removed_ids.is_empty() {
            self.with(|inner| {
                for id in &removed_ids {
                    inner.record.revoked_installations.insert(id.clone());
                }
                if inner.is_current(signed.epoch) {
                    inner.update(&self.watch, |state| {
                        for id in &removed_ids {
                            state.revoked_installations.insert(id.clone());
                        }
                    });
                }
            });
            self.persist().await;
            self.push().await;
            if let Some(last) = removed_ids.last() {
                self.ports.transport.installation_revoked(last).await;
            }
        }
        Ok(result)
    }
}
