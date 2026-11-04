use super::lifecycle::AccountMesh;
use super::state::{AccountMeshRefusal, EndpointSequence};

impl AccountMesh {
    pub async fn accept_endpoint_sequence(
        &self,
        account_id: &str,
        installation_id: &str,
        sequence: u64,
        digest: [u8; 32],
    ) -> Result<bool, AccountMeshRefusal> {
        let _guard = self.persist_lock.lock().await;
        let epoch = self.state().account_epoch;
        let changed = self.with(|inner| {
            let account = inner
                .state
                .signed_in_account()
                .ok_or(AccountMeshRefusal::Unauthenticated)?
                .to_owned();
            if account != account_id || sequence == 0 || inner.store_unreadable {
                return Err(AccountMeshRefusal::Cancelled);
            }
            if installation_id == self.installation() && sequence < inner.record.endpoint_sequence {
                return Ok(false);
            }
            let entries = inner.record.endpoint_sequences.entry(account).or_default();
            if let Some(known) = entries.get(installation_id) {
                if sequence <= known.sequence {
                    return Ok(sequence == known.sequence && digest == known.digest);
                }
            } else if entries.len() >= 256 {
                return Err(AccountMeshRefusal::Unavailable);
            }
            entries.insert(
                installation_id.to_owned(),
                EndpointSequence { sequence, digest },
            );
            Ok(true)
        })?;
        if !changed {
            return Ok(false);
        }
        let record = self.with(|inner| inner.record.clone());
        self.ports
            .store
            .save(&record)
            .await
            .map_err(|_| AccountMeshRefusal::Unavailable)?;
        if self.state().account_epoch != epoch {
            return Err(AccountMeshRefusal::Cancelled);
        }
        Ok(true)
    }
}
