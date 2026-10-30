use super::key::sha256_hex;
use super::membership::membership_token;
use super::wire::{Eligibility, MeshOperation, MeshResult, RegistryPresence};
use super::{AccountMesh, AccountMeshRefusal, LinkAuthorization, Presence};

type Outcome<T> = Result<T, AccountMeshRefusal>;

impl AccountMesh {
    /// Fresh online, per-side authorization for a NEW link; never served from any cache.
    /// The peer target only names the request; the server re-checks it against the registry.
    pub async fn authorize_link(&self, request: LinkAuthorization) -> Outcome<String> {
        let now = self.now();
        let epoch = self.with(|inner| {
            let state = &inner.state;
            match &state.presence {
                Presence::SignedOut => Err(AccountMeshRefusal::SignedOut),
                Presence::NeverSignedIn => Err(AccountMeshRefusal::Unauthenticated),
                Presence::SignedIn { account_id } if *account_id != request.account_id => {
                    Err(AccountMeshRefusal::Unauthenticated)
                }
                Presence::SignedIn { .. } if state.plan_limit_locked() => {
                    Err(AccountMeshRefusal::PlanLimitLocked)
                }
                Presence::SignedIn { .. } if state.current_membership(now).is_none() => {
                    Err(AccountMeshRefusal::Revoked)
                }
                Presence::SignedIn { .. } => Ok(state.account_epoch),
            }
        })?;
        if request.local_installation_id != self.installation() {
            return Err(AccountMeshRefusal::Invalid);
        }
        let resolved = self.with(|inner| {
            inner.resolved.get(&request.peer_installation_id).cloned().or_else(|| {
                inner.state.registry.as_ref()?.rows.iter()
                    .find(|row| row.installation_id == request.peer_installation_id && row.active
                        && row.eligibility == Eligibility::Eligible && row.presence == RegistryPresence::Present)
                    .map(|row| row.target())
            })
        });
        // A responder authorizes on the initiator's slot identity; it never dials that peer.
        let cached = resolved.is_some();
        let peer = match resolved {
            Some(peer) => peer,
            None => {
                self.resolve_link_identity(&request.peer_installation_id)
                    .await?
            }
        };
        let signed = self.signed(Some(epoch)).await?;
        let expected_slot_revision = self
            .own_slot(&signed.account)
            .ok_or(AccountMeshRefusal::Revoked)?;
        let transcript_sha256 = sha256_hex(&request.transcript);
        let operation = |peer| MeshOperation::Link {
            expected_slot_revision,
            peer,
            transcript_sha256: transcript_sha256.clone(),
        };
        let result = match self.admission(&signed, operation(peer)).await {
            Err(AccountMeshRefusal::Conflict | AccountMeshRefusal::Forbidden) if cached => {
                self.with(|inner| { inner.resolved.remove(&request.peer_installation_id); });
                let peer = self.resolve_link_identity(&request.peer_installation_id).await?;
                self.admission(&signed, operation(peer)).await?
            }
            result => result?,
        };
        let MeshResult::Permit(result) = result else {
            return Err(AccountMeshRefusal::Invalid);
        };
        if result.installation_id != self.installation()
            || result.peer_installation_id != request.peer_installation_id
        {
            return Err(AccountMeshRefusal::Invalid);
        }
        if !self.with(|inner| inner.is_current(epoch)) {
            return Err(AccountMeshRefusal::Cancelled);
        }
        Ok(membership_token(&result.permit))
    }

}
