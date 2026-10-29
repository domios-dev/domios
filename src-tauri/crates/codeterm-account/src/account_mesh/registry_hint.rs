use super::notice::{Claim, RegistryNotices};
use super::{AccountMesh, AccountMeshRefusal};

#[derive(Default)]
pub(super) struct RegistryHints {
    epoch: u64,
    notices: RegistryNotices,
    revision: u64,
    forced: bool,
}

struct Flight<'a> {
    mesh: &'a AccountMesh,
    epoch: u64,
    active: bool,
}
impl Drop for Flight<'_> {
    fn drop(&mut self) {
        if self.active {
            self.mesh
                .with(|inner| inner.registry_hints.notices.stop(self.epoch));
        }
    }
}

impl AccountMesh {
    pub async fn registry_hint(
        &self,
        sender: &str,
        nonce: &str,
        account: &str,
        revision: Option<u64>,
        security_digest: Option<&str>,
    ) -> Result<bool, AccountMeshRefusal> {
        let (epoch, claim) = self.with(|inner| {
            if inner.state.signed_in_account() != Some(account) {
                return Err(AccountMeshRefusal::Forbidden);
            }
            let observed = inner
                .state
                .registry_security
                .as_ref()
                .map_or(0, |security| security.revision);
            if revision.is_some() && security_digest.is_none() {
                return Ok((inner.state.account_epoch, Claim::Ignore));
            }
            if security_digest.is_some_and(|digest| {
                inner
                    .state
                    .registry_security
                    .as_ref()
                    .is_some_and(|security| security.digest == digest)
            }) {
                return Ok((inner.state.account_epoch, Claim::Ignore));
            }
            if revision.is_some_and(|revision| revision <= observed) {
                return Ok((inner.state.account_epoch, Claim::Ignore));
            }
            let epoch = inner.state.account_epoch;
            let hints = &mut inner.registry_hints;
            if hints.epoch != epoch {
                *hints = RegistryHints {
                    epoch,
                    ..Default::default()
                };
            }
            let claim = hints.notices.claim(epoch, sender, nonce, 0, self.now());
            if claim != Claim::Ignore {
                hints.revision = hints.revision.max(revision.unwrap_or(0));
                hints.forced |= revision.is_none();
            }
            Ok((epoch, claim))
        })?;
        if claim != Claim::Run {
            return Ok(false);
        }
        let mut flight = Flight {
            mesh: self,
            epoch,
            active: true,
        };
        loop {
            let needed = self.with(|inner| {
                if !inner.is_current(epoch) {
                    return Err(AccountMeshRefusal::Cancelled);
                }
                let observed = inner
                    .state
                    .registry_security
                    .as_ref()
                    .map_or(0, |registry| registry.revision);
                let hints = &mut inner.registry_hints;
                let needed = std::mem::take(&mut hints.forced) || hints.revision > observed;
                if !needed {
                    hints.notices.stop(epoch);
                    flight.active = false;
                }
                Ok(needed)
            })?;
            if !needed {
                return Ok(true);
            }
            self.list().await?;
            let observed = self
                .state()
                .registry_security
                .as_ref()
                .map_or(0, |registry| registry.revision);
            if self.with(|inner| inner.registry_hints.revision > observed) {
                return Err(AccountMeshRefusal::Conflict);
            }
        }
    }
}
