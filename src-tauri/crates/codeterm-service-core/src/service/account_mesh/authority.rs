use codeterm_account::account_mesh as account;
use codeterm_account::account_mesh::{AccountMesh, AccountMeshRefusal, MembershipRefusal};
use codeterm_mesh::account_mesh as mesh;
use std::sync::Arc;

/// The account lifecycle as the Mesh gate's authority; it never answers from a cache.
pub struct AccountAuthority(pub Arc<AccountMesh>);

pub fn mesh_state(state: &account::LifecycleState) -> mesh::AccountMeshState {
    mesh::AccountMeshState {
        account_epoch: state.account_epoch,
        presence: match &state.presence {
            account::Presence::NeverSignedIn => mesh::AccountPresence::NeverSignedIn,
            account::Presence::SignedIn { account_id } => mesh::AccountPresence::SignedIn {
                account_id: account_id.clone(),
            },
            account::Presence::SignedOut => mesh::AccountPresence::SignedOut,
        },
        membership: state
            .membership
            .as_ref()
            .map(|membership| mesh::SignedMembership {
                token: membership.token.clone(),
                verified: verified(&membership.verified),
            }),
        revoked_installations: state.revoked_installations.clone(),
        plan_limit_locked: state.plan_limit_locked(),
    }
}

fn verified(value: &account::VerifiedMembership) -> mesh::VerifiedMembership {
    mesh::VerifiedMembership {
        account_id: value.account_id.clone(),
        installation_id: value.installation_id.clone(),
        key_id: value.key_id.clone(),
        key_generation: value.key_generation,
        slot_revision: value.slot_revision,
        issued_at: value.issued_at,
        expires_at: value.expires_at,
    }
}

fn reason(refusal: &impl serde::Serialize) -> String {
    match serde_json::to_value(refusal) {
        Ok(serde_json::Value::String(code)) => code,
        Ok(value) => value
            .get("reason")
            .and_then(|reason| reason.as_str())
            .unwrap_or("unavailable")
            .to_owned(),
        Err(_) => "unavailable".to_owned(),
    }
}

pub fn admission_refusal(refusal: AccountMeshRefusal) -> mesh::AdmissionRefusal {
    use mesh::AdmissionRefusal as M;
    match refusal {
        AccountMeshRefusal::Offline
        | AccountMeshRefusal::Unavailable
        | AccountMeshRefusal::AuthorityUnavailable => M::Offline,
        AccountMeshRefusal::Unauthenticated | AccountMeshRefusal::AppCheck => M::Unauthenticated,
        AccountMeshRefusal::MachineLimit {
            limit,
            active_count,
        } => M::MachineLimit {
            limit,
            active_count,
        },
        AccountMeshRefusal::RateLimited {
            retry_after_seconds,
        } => M::RateLimited {
            retry_after_seconds,
        },
        AccountMeshRefusal::Revoked | AccountMeshRefusal::Forbidden => M::Revoked,
        AccountMeshRefusal::SignedOut => M::SignedOut,
        AccountMeshRefusal::PlanLimitLocked => M::PlanLimitLocked,
        AccountMeshRefusal::Cancelled => M::Cancelled,
        AccountMeshRefusal::Invalid
        | AccountMeshRefusal::Conflict
        | AccountMeshRefusal::VerificationUnavailable => M::handshake(reason(&refusal)),
    }
}

/// Expiry is a revocation boundary; every other verification failure, including a
/// not-yet-valid one beyond the skew leeway, fails the handshake.
pub fn membership_refusal(refusal: MembershipRefusal) -> mesh::AdmissionRefusal {
    match refusal {
        MembershipRefusal::Expired => mesh::AdmissionRefusal::Revoked,
        other => mesh::AdmissionRefusal::handshake(format!("membership-{}", reason(&other))),
    }
}

impl mesh::AccountMeshAuthority for AccountAuthority {
    fn verify_membership(
        &self,
        token: &str,
        now: u64,
    ) -> Result<mesh::VerifiedMembership, mesh::AdmissionRefusal> {
        self.0
            .verify_peer_membership(token, now)
            .map(|value| verified(&value))
            .map_err(membership_refusal)
    }

    fn sign_possession(&self, transcript: &[u8; 32]) -> Result<String, mesh::AdmissionRefusal> {
        self.0
            .sign_possession(transcript)
            .map_err(admission_refusal)
    }

    fn verify_possession(
        &self,
        membership: &mesh::VerifiedMembership,
        transcript: &[u8; 32],
        signature: &str,
    ) -> bool {
        account::verify_possession(&membership.key_id, transcript, signature)
    }

    fn authorize_link(
        &self,
        request: mesh::LinkAuthorizationRequest,
    ) -> mesh::BoxFuture<'_, Result<String, mesh::AdmissionRefusal>> {
        Box::pin(async move {
            self.0
                .authorize_link(account::LinkAuthorization {
                    transcript: request.transcript,
                    local_installation_id: request.local_installation_id,
                    peer_installation_id: request.peer_installation_id,
                    account_id: request.account_id,
                })
                .await
                .map_err(admission_refusal)
        })
    }

    fn verify_link_permit(
        &self,
        permit: &str,
        expected: &mesh::LinkPermitExpectation,
        now: u64,
    ) -> Result<(), mesh::AdmissionRefusal> {
        let expectation = account::LinkPermitExpectation {
            transcript: &expected.transcript,
            holder_installation_id: &expected.holder_installation_id,
            peer_installation_id: &expected.peer_installation_id,
            account_id: &expected.account_id,
            peer_target: None,
        };
        self.0
            .verify_peer_link_permit(permit, &expectation, now)
            .map_err(membership_refusal)
    }

    fn resolve_link_target(
        &self,
        installation_id: &str,
    ) -> mesh::BoxFuture<'_, Result<mesh::LinkTarget, mesh::AdmissionRefusal>> {
        let installation_id = installation_id.to_owned();
        Box::pin(async move {
            let target = self
                .0
                .resolve_link_target(&installation_id)
                .await
                .map_err(admission_refusal)?;
            Ok(mesh::LinkTarget {
                installation_id: target.target.installation_id,
                ip: target.endpoint.ip().to_string(),
                api_port: target.endpoint.port(),
            })
        })
    }
}
