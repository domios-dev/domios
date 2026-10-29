use super::lifecycle::AccountMesh;
use super::operations::Signed;
use super::state::{AccountMeshRefusal, Enrollment};
use super::wire::{InstallationEnrollment, KeyEnrollment};

impl AccountMesh {
    pub(super) async fn require_sign_in(&self, epoch: u64) -> AccountMeshRefusal {
        let refusal = self.with(|inner| {
            if !inner.is_current(epoch) {
                return AccountMeshRefusal::Cancelled;
            }
            let Some(account) = inner.state.signed_in_account().map(str::to_owned) else {
                return AccountMeshRefusal::Cancelled;
            };
            inner.record.reauth_required_accounts.insert(account);
            inner.record.admission_cache = None;
            inner.retry = Default::default();
            inner.pending_renew = None;
            inner.lease += 1;
            inner.update(&self.watch, |state| {
                state.membership = None;
                state.enrollment = Enrollment::Refused;
                state.last_refusal = Some(AccountMeshRefusal::ReauthRequired);
                state.retry_at = None;
            });
            AccountMeshRefusal::ReauthRequired
        });
        self.persist().await;
        self.push().await;
        refusal
    }

    pub(super) async fn enroll_installation_key(
        &self,
        signed: &Signed,
        request: &InstallationEnrollment,
    ) -> Result<KeyEnrollment, AccountMeshRefusal> {
        match self
            .call(self.ports.registry.enroll_key(&signed.session, request))
            .await
        {
            Err(AccountMeshRefusal::GenerationMismatch) => {
                Err(self.require_sign_in(signed.epoch).await)
            }
            result => result,
        }
    }
}
