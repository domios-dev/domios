use axum::http::StatusCode;

/// No paired record at all for this id/name — distinct from [`PEER_UNROUTABLE_REASON`],
/// which means the record exists but `client_for` couldn't build a route to it.
pub(super) const PEER_NOT_PAIRED_REASON: &str = "peer_not_paired";
/// The paired record exists, but `MeshManager::client_for` found no route.
/// Keep this distinct from a missing record (codeterm#368).
pub(super) const PEER_UNROUTABLE_REASON: &str = "peer_unroutable";
/// A name matched one or more paired records, but none is online — refusing
/// to guess between them (codeterm#434). Distinct from [`PEER_NOT_PAIRED_REASON`]
/// (no record at all) and from the per-peer protocol mismatch below (exactly
/// one online-but-outdated record).
pub(super) const PEER_NAME_NO_ONLINE_RECORD_REASON: &str = "peer_name_no_online_record";
/// More than one online paired record answers to the target — the caller
/// must re-address by instance id.
pub(super) const PEER_NAME_AMBIGUOUS_REASON: &str = "peer_name_ambiguous";

pub(super) fn peer_not_paired_error(target: &str) -> (StatusCode, String) {
    (
        StatusCode::NOT_FOUND,
        format!("{PEER_NOT_PAIRED_REASON}: {target} is not a paired peer"),
    )
}

pub(super) fn peer_unroutable_error(peer_name: &str) -> (StatusCode, String) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        format!(
            "{PEER_UNROUTABLE_REASON}: peer {peer_name} is paired but has no reachable address"
        ),
    )
}

pub(super) fn candidates_refusal(
    status: StatusCode,
    reason: &str,
    name: &str,
    candidates: &[crate::mesh::paired_resolve::PairedNameCandidate],
) -> (StatusCode, String) {
    let mut candidates: Vec<_> = candidates.to_vec();
    candidates.sort_by(|a, b| a.instance_id.cmp(&b.instance_id));
    let body = serde_json::json!({
        "reason": reason,
        "name": name,
        "candidates": candidates.iter().map(|c| serde_json::json!({
            "instance_id": c.instance_id,
            "instance_name": c.instance_name,
            "online": c.online,
            "relay_protocol_version": c.relay_protocol_version,
            "route_by_id": { "target_peer_id": c.instance_id },
        })).collect::<Vec<_>>(),
    });
    (
        status,
        serde_json::to_string(&body).unwrap_or_else(|_| reason.to_string()),
    )
}

/// Refusal for a name that matched only offline/dead records. Names EVERY
/// candidate considered plus the id form of the same relay call, as
/// structured JSON — never prose a caller would have to pattern-match.
pub(super) fn peer_name_no_online_record_error(
    name: &str,
    candidates: &[crate::mesh::paired_resolve::PairedNameCandidate],
) -> (StatusCode, String) {
    candidates_refusal(
        StatusCode::SERVICE_UNAVAILABLE,
        PEER_NAME_NO_ONLINE_RECORD_REASON,
        name,
        candidates,
    )
}

pub(super) fn peer_name_ambiguous_error(
    name: &str,
    candidates: &[crate::mesh::paired_resolve::PairedNameCandidate],
) -> (StatusCode, String) {
    candidates_refusal(
        StatusCode::CONFLICT,
        PEER_NAME_AMBIGUOUS_REASON,
        name,
        candidates,
    )
}

pub(super) type ResolvedRelayPeer = (
    crate::mesh::transport::PeerClient,
    String,
    String,
    Option<String>,
);

/// A relay answer that names the peer instance that produced it, so a caller
/// addressing by name learns which machine actually answered.
#[derive(serde::Serialize)]
pub(crate) struct PeerRelayResponse {
    #[serde(flatten)]
    pub(super) response: crate::mesh::types::RelayResponse,
    pub(super) peer: AnsweringPeer,
}

#[derive(serde::Serialize)]
pub(crate) struct AnsweringPeer {
    pub(super) instance_id: String,
    pub(super) instance_name: String,
}

/// Resolve a paired target while preserving missing, incompatible, and
/// currently-unroutable refusal states. Name resolution over duplicate
/// records sharing an `instance_name` prefers the online, highest-protocol
/// record deterministically (codeterm#434) — see
/// [`crate::mesh::paired_resolve::resolve_paired_by_name`].
pub(super) fn resolve_relay_peer(
    mesh: &crate::mesh::MeshManager,
    target: &str,
) -> Result<ResolvedRelayPeer, (StatusCode, String)> {
    let peer = crate::mesh::paired_resolve::resolve_paired_by_name(&mesh.paired_peers, target)
        .map_err(|error| match error {
            crate::mesh::paired_resolve::ResolvePairedError::NotFound => {
                peer_not_paired_error(target)
            }
            crate::mesh::paired_resolve::ResolvePairedError::NoOnlineCandidate {
                name,
                candidates,
            } => peer_name_no_online_record_error(&name, &candidates),
            crate::mesh::paired_resolve::ResolvePairedError::Ambiguous { name, candidates } => {
                peer_name_ambiguous_error(&name, &candidates)
            }
        })?;
    if peer.relay_protocol_version != crate::mesh::types::RELAY_PROTOCOL_VERSION {
        return Err((
            StatusCode::UPGRADE_REQUIRED,
            format!(
                "Peer {} uses incompatible Mesh relay protocol {}",
                peer.instance_name, peer.relay_protocol_version
            ),
        ));
    }
    let name = peer.instance_name.clone();
    let version = peer.version.clone();
    let instance_id = peer.instance_id.clone();
    let client = mesh
        .client_for(&instance_id)
        .ok_or_else(|| peer_unroutable_error(&name))?;
    Ok((client, instance_id, name, version))
}
