use base64::Engine;

pub(super) fn presented_kind(token: &str) -> &'static str {
    if token.is_empty() {
        return "missing";
    }
    if token.len() <= 4096 {
        if let Some((payload, signature)) = token.split_once('.') {
            let base64 = base64::engine::general_purpose::STANDARD;
            let request = base64.decode(payload).ok().and_then(|bytes| {
                serde_json::from_slice::<codeterm_service_core::mesh::crown::SignedRequest>(&bytes)
                    .ok()
            });
            if request.is_some()
                && base64
                    .decode(signature)
                    .is_ok_and(|bytes| bytes.len() == 64)
            {
                return "mesh-signature";
            }
        }
    }
    "bearer"
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum MeshRefusal {
    NotReady,
    PlanLimitLocked(String),
    Unauthorized(&'static str),
}

pub(super) fn classify_verify_error(error: &str) -> &'static str {
    match error {
        "Request expired" => "clock_skew_expired",
        "Request timestamp too far in the future" => "clock_skew_future",
        _ => "unknown_or_invalid_credential",
    }
}

pub(super) fn classify_mesh_refusal(state: &crate::routes::AppState, token: &str) -> MeshRefusal {
    use codeterm_service_core::mesh::account_mesh::gate::{BlockReason, TrafficDecision};
    if token.is_empty() {
        return MeshRefusal::Unauthorized("unknown_or_invalid_credential");
    }
    // Clone keys before Ed25519 verification so refusal storms never hold the mesh mutex.
    let (account_ready, paired_keys) = {
        let mesh = state.mesh.lock();
        (mesh.is_account_ready(), mesh.paired_identity_keys())
    };
    if !account_ready {
        return MeshRefusal::NotReady;
    }
    let peer = match codeterm_service_core::mesh::crown::verify_signed_request(token, &paired_keys)
    {
        Ok(peer) => peer,
        Err(error) => return MeshRefusal::Unauthorized(classify_verify_error(&error)),
    };
    match state.mesh.lock().traffic_decision(&peer) {
        TrafficDecision::Blocked(BlockReason::AccountPending) => MeshRefusal::NotReady,
        TrafficDecision::Blocked(BlockReason::PlanLimitLocked) => {
            MeshRefusal::PlanLimitLocked(peer)
        }
        TrafficDecision::Blocked(BlockReason::LocalMembershipExpired) => {
            MeshRefusal::Unauthorized("local_membership_expired")
        }
        TrafficDecision::Blocked(
            BlockReason::PeerMembershipExpired | BlockReason::PeerMembershipMissing,
        ) => MeshRefusal::Unauthorized("peer_membership_expired"),
        TrafficDecision::Blocked(BlockReason::Revoked) => MeshRefusal::Unauthorized("revoked"),
        _ => MeshRefusal::Unauthorized("account_admission_blocked"),
    }
}

#[cfg(test)]
mod tests;
