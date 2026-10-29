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
        matches!(state.last_refusal, Some(AccountMeshRefusal::Revoked))
            || state.registry.as_ref().is_some_and(|registry| {
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
        let mut active_target = self
            .state()
            .registry
            .as_ref()
            .and_then(|registry| {
                registry
                    .rows
                    .iter()
                    .find(|row| row.installation_id == self.installation() && row.active)
                    .map(|row| row.target())
            })
            .or_else(|| self.cached_active_target(signed));
        let generation = self.generation(signed).await?;
        if let Some(target) = active_target
            .as_ref()
            .filter(|target| generation > target.key_generation)
        {
            match self
                .rejoin_admission(
                    signed,
                    MeshOperation::Recover {
                        target: target.clone(),
                    },
                )
                .await
            {
                Err(AccountMeshRefusal::Forbidden) => active_target = None,
                result => return result,
            }
        }
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
        if let Some(target) = active_target {
            match self
                .rejoin_admission(signed, MeshOperation::Recover { target })
                .await
            {
                Err(AccountMeshRefusal::Forbidden) => {}
                result => return result,
            }
        }
        self.rejoin_admission(
            signed,
            MeshOperation::Enroll {
                expected_slot_revision: 0,
            },
        )
        .await
    }

    async fn rejoin_admission(
        &self,
        signed: &Signed,
        operation: MeshOperation,
    ) -> Result<MeshResult, AccountMeshRefusal> {
        match self.admission(signed, operation).await? {
            MeshResult::Recovered(result) => {
                Ok(MeshResult::Membership(super::wire::MembershipResult {
                    operation: super::wire::MembershipOperation::Recover,
                    installation_id: result.installation_id,
                    slot_revision: result.slot_revision,
                    registry_revision: result.registry_revision,
                    membership: result.membership.ok_or(AccountMeshRefusal::Invalid)?,
                }))
            }
            result => Ok(result),
        }
    }
}
