use super::key::InstallationKey;
use super::lifecycle::AccountMesh;
use super::operations::Signed;
use super::state::AccountMeshRefusal;
use super::wire::{
    InstallationEnrollment, InstallationKeyBody, KeyOperation, MeshOperation, MeshResult,
    ProofEndpoint,
};
use std::sync::Arc;

impl AccountMesh {
    pub(super) fn own_removed(&self) -> bool {
        let state = self.state();
        matches!(
            state.last_refusal,
            Some(AccountMeshRefusal::Revoked | AccountMeshRefusal::Forbidden)
        ) || state.registry.as_ref().is_some_and(|registry| {
            registry
                .rows
                .iter()
                .any(|row| row.installation_id == self.installation() && !row.active)
        })
    }

    pub(super) async fn rejoin(
        &self,
        signed: &mut Signed,
    ) -> Result<MeshResult, AccountMeshRefusal> {
        self.with(|inner| {
            if !inner.is_current(signed.epoch) {
                return Err(AccountMeshRefusal::Cancelled);
            }
            inner.update(&self.watch, |state| state.membership = None);
            Ok(())
        })?;
        let attempt = signed.epoch;
        self.with(|inner| {
            if !inner.is_current(signed.epoch) {
                return Err(AccountMeshRefusal::Cancelled);
            }
            if inner.rotation_attempt == Some(attempt) {
                return Err(inner
                    .state
                    .last_refusal
                    .unwrap_or(AccountMeshRefusal::Revoked));
            }
            inner.rotation_attempt = Some(attempt);
            Ok(())
        })?;
        let generation = self.generation(signed).await?;
        let _guard = self.key_lock.lock().await;
        let key = Arc::new(InstallationKey::generate());
        let next_generation = generation
            .checked_add(1)
            .ok_or(AccountMeshRefusal::Invalid)?;
        let body = InstallationKeyBody {
            installation_id: self.installation(),
            public_key_spki_base64: key.spki_base64(),
            operation: KeyOperation::Recover,
            expected_generation: generation,
        };
        signed.key = key.clone();
        let proof = self
            .prove(
                signed,
                ProofEndpoint::EnrollInstallation,
                &body,
                next_generation,
            )
            .await?;
        let request = InstallationEnrollment { body, proof };
        let enrolled = self
            .call(self.ports.registry.enroll_key(&signed.session, &request))
            .await?;
        if enrolled.key_id != key.key_id() || enrolled.generation != next_generation {
            return Err(AccountMeshRefusal::Invalid);
        }
        self.ports
            .keys
            .save(&key.to_secret())
            .await
            .map_err(|_| AccountMeshRefusal::Unavailable)?;
        self.with(|inner| {
            inner.key = Some(key);
            inner.record.keys.insert(
                signed.account.clone(),
                super::state::KeyBinding {
                    key_id: enrolled.key_id,
                    generation: enrolled.generation,
                },
            );
            if !inner.is_current(signed.epoch) {
                return Err(AccountMeshRefusal::Cancelled);
            }
            inner.update(&self.watch, |state| state.membership = None);
            Ok(())
        })?;
        self.persist().await;
        drop(_guard);
        self.admission(
            signed,
            MeshOperation::Enroll {
                expected_slot_revision: 0,
            },
        )
        .await
    }
}
