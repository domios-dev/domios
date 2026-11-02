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

pub(super) fn refusal_reason(state: &crate::routes::AppState, token: &str) -> &'static str {
    use codeterm_service_core::mesh::account_mesh::gate::{BlockReason, TrafficDecision};
    let mesh = state.mesh.lock();
    let Ok(peer) = codeterm_service_core::mesh::crown::verify_signed_request(
        token,
        &mesh.paired_identity_keys(),
    ) else {
        return "unknown_or_invalid_credential";
    };
    match mesh.traffic_decision(&peer) {
        TrafficDecision::Blocked(BlockReason::LocalMembershipExpired) => "local_membership_expired",
        TrafficDecision::Blocked(
            BlockReason::PeerMembershipExpired | BlockReason::PeerMembershipMissing,
        ) => "peer_membership_expired",
        TrafficDecision::Blocked(BlockReason::Revoked) => "revoked",
        _ => "account_admission_blocked",
    }
}

#[cfg(test)]
mod tests;
