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
        let target = self.state().registry.as_ref().and_then(|registry| {
            registry.rows.iter().find(|row| row.installation_id == self.installation() && row.active).map(|row| row.target())
        }).or_else(|| self.cached_active_target(signed));
        if !self.own_removed() && !self.with(|inner| inner.rejoin_required) {
            if let Some(target) = target {
                if self.generation(signed).await? > target.key_generation {
                    return self.rejoin_admission(signed, MeshOperation::Recover {
                        target: super::wire::RecoveryTarget::Exact(target), target_policy: None,
                    }).await;
                }
            }
        }
        let allowed = self.with(|inner| {
            if !inner.is_current(signed.epoch) {
                return Err(AccountMeshRefusal::Cancelled);
            }
            inner.rejoin_required = true;
            if !inner.rejoin_authorized || inner.rotation_attempt == Some(signed.epoch) {
                return Err(inner.state.last_refusal.unwrap_or(AccountMeshRefusal::ReauthRequired));
            }
            inner.rotation_attempt = Some(signed.epoch);
            inner.rejoin_authorized = false;
            inner.update(&self.watch, |state| state.membership = None);
            Ok(())
        });
        allowed?;
        let existing = self.with(|inner| inner.record.keys.contains_key(&signed.account) || inner.record.slots.contains_key(&signed.account));
        let _guard = self.key_lock.lock().await;
        let key = Arc::new(InstallationKey::generate());
        let body = InstallationKeyBody {
            installation_id: self.installation(),
            public_key_spki_base64: key.spki_base64(),
            operation: KeyOperation::Recover,
            expected_generation: 0,
            generation_policy: Some(super::wire::GenerationPolicy::Assign),
        };
        signed.key = key.clone();
        let proof = self
            .prove(
                signed,
                ProofEndpoint::EnrollInstallation,
                &body,
                0,
            )
            .await?;
        let request = InstallationEnrollment { body, proof };
        let enrolled = self
            .call(self.ports.registry.enroll_key(&signed.session, &request))
            .await.map_err(|refusal| match refusal {
                AccountMeshRefusal::ProofInvalid => AccountMeshRefusal::ReauthRequired,
                other => other,
            })?;
        if enrolled.key_id != key.key_id() || enrolled.generation == 0 {
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
        let operation = if existing {
            MeshOperation::Recover {
                target: super::wire::RecoveryTarget::Own { installation_id: self.installation() },
                target_policy: Some(super::wire::OwnTargetPolicy::Own),
            }
        } else {
            MeshOperation::Enroll { expected_slot_revision: 0 }
        };
        self.rejoin_admission(signed, operation)
        .await
    }

    async fn rejoin_admission(
        &self,
        signed: &Signed,
        operation: MeshOperation,
    ) -> Result<MeshResult, AccountMeshRefusal> {
        let own = matches!(operation, MeshOperation::Recover { target_policy: Some(_), .. });
        match self.admission(signed, operation).await? {
            MeshResult::Membership(result) => Ok(MeshResult::Membership(result)),
            MeshResult::Recovered(result) if !own => {
                Ok(MeshResult::Membership(super::wire::MembershipResult {
                    operation: super::wire::MembershipOperation::Recover,
                    installation_id: result.installation_id,
                    slot_revision: result.slot_revision,
                    registry_revision: result.registry_revision,
                    membership: result.membership.ok_or(AccountMeshRefusal::Invalid)?,
                }))
            }
            _ => Err(AccountMeshRefusal::Invalid),
        }
    }
}
