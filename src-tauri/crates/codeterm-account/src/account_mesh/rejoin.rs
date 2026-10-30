use super::key::InstallationKey;
use super::lifecycle::AccountMesh;
use super::operations::Signed;
use super::state::AccountMeshRefusal;
use super::wire::{
    InstallationEnrollment, InstallationKeyBody, KeyOperation, MeshOperation, MeshResult,
    ProofEndpoint,
};
use std::sync::Arc;

const RECOVERY_REPLAY_SECONDS: u64 = 120 - 30;

pub(super) struct PreparedRecovery {
    pub envelope: super::wire::SignedEnvelope<super::wire::MeshAdmissionBody>,
    pub issued_at: u64,
}

impl PreparedRecovery {
    pub fn replayable(&self, now: u64) -> bool {
        now < self.issued_at.saturating_add(RECOVERY_REPLAY_SECONDS)
    }
}

pub(super) struct RejoinWindow {
    pub epoch: u64,
    pub signed_in_at: u64,
    pub rotated: Option<u64>,
    pub pending_key: Option<Arc<InstallationKey>>,
    pub recovery: Option<PreparedRecovery>,
    pub recovery_committed: bool,
    pub existing: bool,
}

impl RejoinWindow {
    pub fn live(&self, epoch: u64, now: u64) -> bool {
        self.epoch == epoch && now < self.expires_at()
    }

    pub fn expires_at(&self) -> u64 {
        self.signed_in_at.saturating_add(900)
    }
}

fn transient(refusal: AccountMeshRefusal) -> bool {
    matches!(refusal, AccountMeshRefusal::Conflict | AccountMeshRefusal::RateLimited { .. }
        | AccountMeshRefusal::EnrollmentBudget { .. }
        | AccountMeshRefusal::Offline | AccountMeshRefusal::Unavailable
        | AccountMeshRefusal::AuthorityUnavailable)
}

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
        let result = self.resume_rejoin(signed).await;
        self.with(|inner| {
            if inner.is_current(signed.epoch) {
                inner.rejoin_authorized = result.as_ref().err().is_some_and(|refusal| transient(*refusal))
                    && inner.rejoin_window.as_ref().is_some_and(|window| window.live(signed.epoch, self.now()));
            }
        });
        result
    }

    async fn resume_rejoin(&self, signed: &mut Signed) -> Result<MeshResult, AccountMeshRefusal> {
        let (rotated, existing, key) = self.with(|inner| {
            if !inner.is_current(signed.epoch) {
                return Err(AccountMeshRefusal::Cancelled);
            }
            inner.rejoin_required = true;
            let window = inner.rejoin_window.as_mut()
                .filter(|window| window.live(signed.epoch, self.now()))
                .ok_or(AccountMeshRefusal::ReauthRequired)?;
            if !inner.rejoin_authorized {
                return Err(inner.state.last_refusal.unwrap_or(AccountMeshRefusal::ReauthRequired));
            }
            let key = window.pending_key.get_or_insert_with(|| Arc::new(InstallationKey::generate())).clone();
            let rotated = window.rotated;
            let existing = window.existing;
            inner.rotation_attempt = Some(signed.epoch);
            inner.rejoin_authorized = false;
            inner.update(&self.watch, |state| state.membership = None);
            Ok((rotated, existing, key))
        })?;
        signed.key = key.clone();
        if let Some(generation) = rotated {
            let stored = self.with(|inner| inner.record.keys.get(&signed.account).cloned());
            if !stored.is_some_and(|binding| binding.generation == generation && binding.key_id == key.key_id()) {
                return Err(AccountMeshRefusal::Invalid);
            }
        } else {
            self.assign_rejoin_key(signed, key).await?;
        }
        if !self.with(|inner| inner.is_current(signed.epoch)
            && inner.rejoin_window.as_ref().is_some_and(|window| window.live(signed.epoch, self.now()))) {
            return Err(AccountMeshRefusal::ReauthRequired);
        }
        let operation = if existing {
            MeshOperation::Recover {
                target: super::wire::RecoveryTarget::Own { installation_id: self.installation() },
                target_policy: Some(super::wire::OwnTargetPolicy::Own),
            }
        } else {
            MeshOperation::Enroll { expected_slot_revision: 0 }
        };
        match self.rejoin_admission(signed, operation).await {
            Err(AccountMeshRefusal::ChallengeInvalid | AccountMeshRefusal::Forbidden) if existing => {
                self.reconcile_rejoin(signed).await
            }
            result => result,
        }
    }

    async fn reconcile_rejoin(&self, signed: &Signed) -> Result<MeshResult, AccountMeshRefusal> {
        let registry = self.list_fresh().await.map_err(|refusal| match refusal {
            AccountMeshRefusal::Forbidden | AccountMeshRefusal::Revoked
            | AccountMeshRefusal::GenerationMismatch | AccountMeshRefusal::KeyConflict
            | AccountMeshRefusal::ChallengeInvalid => AccountMeshRefusal::ReauthRequired,
            other => other,
        })?;
        let generation = self.with(|inner| inner.rejoin_window.as_ref().and_then(|window| window.rotated));
        let own = registry.rows.iter().find(|row| row.installation_id == self.installation()
            && row.active && Some(row.key_generation) == generation && row.key_id == signed.key.key_id())
            .ok_or(AccountMeshRefusal::ReauthRequired)?;
        self.with(|inner| {
            if !inner.is_current(signed.epoch) { return Err(AccountMeshRefusal::Cancelled); }
            inner.record.slots.insert(signed.account.clone(), own.slot_revision);
            inner.rejoin_required = false;
            if let Some(window) = inner.rejoin_window.as_mut() { window.recovery_committed = true; }
            inner.update(&self.watch, |state| state.self_installation = Some(super::state::SelfInstallation {
                installation_id: own.installation_id.clone(), key_id: own.key_id.clone(),
                key_generation: own.key_generation, slot_revision: own.slot_revision,
                metadata_revision: own.metadata_revision, display_name: own.display_name.clone(),
            }));
            Ok(())
        })?;
        self.persist().await;
        self.ports.transport.installation_revoked(&self.installation()).await;
        self.admission(signed, MeshOperation::Renew { expected_slot_revision: own.slot_revision }).await
    }

    async fn assign_rejoin_key(&self, signed: &Signed, key: Arc<InstallationKey>) -> Result<(), AccountMeshRefusal> {
        let _guard = self.key_lock.lock().await;
        let body = InstallationKeyBody {
            installation_id: self.installation(),
            public_key_spki_base64: key.spki_base64(),
            operation: KeyOperation::Recover,
            expected_generation: 0,
            generation_policy: Some(super::wire::GenerationPolicy::Assign),
        };
        let proof = self.prove(signed, ProofEndpoint::EnrollInstallation, &body, 0).await?;
        let request = InstallationEnrollment { body, proof };
        let enrolled = self.call(self.ports.registry.enroll_key(&signed.session, &request)).await
            .map_err(|refusal| match refusal {
                AccountMeshRefusal::ProofInvalid => AccountMeshRefusal::ReauthRequired,
                other => other,
            })?;
        if enrolled.key_id != key.key_id() || enrolled.generation == 0 {
            return Err(AccountMeshRefusal::Invalid);
        }
        self.ports.keys.save(&key.to_secret()).await.map_err(|_| AccountMeshRefusal::Unavailable)?;
        self.with(|inner| {
            inner.key = Some(key);
            inner.record.keys.insert(signed.account.clone(), super::state::KeyBinding {
                key_id: enrolled.key_id, generation: enrolled.generation,
            });
            if let Some(window) = inner.rejoin_window.as_mut().filter(|window| window.epoch == signed.epoch) {
                window.rotated = Some(enrolled.generation);
            }
        });
        self.persist().await;
        if !self.with(|inner| inner.is_current(signed.epoch)) {
            return Err(AccountMeshRefusal::Cancelled);
        }
        Ok(())
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
