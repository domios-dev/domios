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

fn registry_peer_unreachable_error(target: &str) -> (StatusCode, String) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        serde_json::json!({
            "code": codeterm_service_core::mesh::contract::delivery::PEER_UNREACHABLE,
            "message": format!("Peer {target} is paired but not currently connected"),
            "bytes_written": false,
            "hint": "Check `domios mesh peers` for connection progress.",
        })
        .to_string(),
    )
}

fn registry_contains_target(mesh: &codeterm_service_core::mesh::MeshManager, target: &str) -> bool {
    let is_short_id =
        target.len() >= 6 && target.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    mesh.account_installations.iter().any(|row| {
        row.active
            && (row.installation_id == target
                || row.instance_id.as_deref() == Some(target)
                || row.name.eq_ignore_ascii_case(target)
                || (is_short_id
                    && row
                        .installation_id
                        .to_ascii_lowercase()
                        .starts_with(&target.to_ascii_lowercase())))
    })
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
    candidates: &[codeterm_service_core::mesh::paired_resolve::PairedNameCandidate],
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
    candidates: &[codeterm_service_core::mesh::paired_resolve::PairedNameCandidate],
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
    candidates: &[codeterm_service_core::mesh::paired_resolve::PairedNameCandidate],
) -> (StatusCode, String) {
    candidates_refusal(
        StatusCode::CONFLICT,
        PEER_NAME_AMBIGUOUS_REASON,
        name,
        candidates,
    )
}

#[cfg(test)]
pub(super) type ResolvedRelayPeer = (
    codeterm_service_core::mesh::transport::PeerClient,
    String,
    String,
    Option<String>,
);

pub(super) type ResolvedRelayRoute = ((String, Vec<String>), String, String, Option<String>);

/// A relay answer that names the peer instance that produced it, so a caller
/// addressing by name learns which machine actually answered.
#[derive(serde::Serialize)]
pub struct PeerRelayResponse {
    #[serde(flatten)]
    pub(super) response: codeterm_service_core::mesh::types::RelayResponse,
    pub(super) peer: AnsweringPeer,
}

#[derive(serde::Serialize)]
pub(crate) struct AnsweringPeer {
    pub(super) instance_id: String,
    pub(super) instance_name: String,
}

/// Resolve a paired target's route endpoints without signing under the mesh lock.
pub(super) fn resolve_relay_peer_route(
    mesh: &codeterm_service_core::mesh::MeshManager,
    target: &str,
) -> Result<ResolvedRelayRoute, (StatusCode, String)> {
    let peer = codeterm_service_core::mesh::paired_resolve::resolve_paired_by_name(
        &mesh.paired_peers,
        target,
    )
    .map_err(|error| match error {
        codeterm_service_core::mesh::paired_resolve::ResolvePairedError::NotFound => {
            if registry_contains_target(mesh, target) {
                registry_peer_unreachable_error(target)
            } else {
                peer_not_paired_error(target)
            }
        }
        codeterm_service_core::mesh::paired_resolve::ResolvePairedError::NoOnlineCandidate {
            name,
            candidates,
        } => peer_name_no_online_record_error(&name, &candidates),
        codeterm_service_core::mesh::paired_resolve::ResolvePairedError::Ambiguous {
            name,
            candidates,
        } => peer_name_ambiguous_error(&name, &candidates),
    })?;
    relay_peer_route(mesh, peer)
}

pub(super) fn resolve_exact_relay_peer_route(
    mesh: &codeterm_service_core::mesh::MeshManager,
    instance_id: &str,
) -> Result<ResolvedRelayRoute, (StatusCode, String)> {
    let peer = mesh.paired_peers.get(instance_id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            serde_json::json!({
                "code": PEER_NOT_PAIRED_REASON,
                "message": "The requested installation is no longer paired",
                "bytes_written": false,
            })
            .to_string(),
        )
    })?;
    relay_peer_route(mesh, peer)
}

#[cfg(test)]
pub(super) fn resolve_relay_peer(
    mesh: &codeterm_service_core::mesh::MeshManager,
    target: &str,
) -> Result<ResolvedRelayPeer, (StatusCode, String)> {
    let ((primary, alternatives), instance_id, name, version) =
        resolve_relay_peer_route(mesh, target)?;
    let token = mesh
        .sign_request()
        .map_err(|_| peer_unroutable_error(&name))?;
    let client = codeterm_service_core::mesh::transport::PeerClient::new_signed_route(
        primary,
        alternatives,
        &token,
    );
    Ok((client, instance_id, name, version))
}

#[cfg(test)]
pub(super) fn resolve_exact_relay_peer(
    mesh: &codeterm_service_core::mesh::MeshManager,
    instance_id: &str,
) -> Result<ResolvedRelayPeer, (StatusCode, String)> {
    let ((primary, alternatives), instance_id, name, version) =
        resolve_exact_relay_peer_route(mesh, instance_id)?;
    let token = mesh
        .sign_request()
        .map_err(|_| peer_unroutable_error(&name))?;
    let client = codeterm_service_core::mesh::transport::PeerClient::new_signed_route(
        primary,
        alternatives,
        &token,
    );
    Ok((client, instance_id, name, version))
}

fn relay_peer_route(
    mesh: &codeterm_service_core::mesh::MeshManager,
    peer: &codeterm_service_core::mesh::types::PairedPeer,
) -> Result<ResolvedRelayRoute, (StatusCode, String)> {
    if !codeterm_service_core::mesh::types::is_supported_relay_protocol(peer.relay_protocol_version)
    {
        return Err(super::origin::protocol_refusal(
            &peer.instance_name,
            peer.relay_protocol_version,
        ));
    }
    let name = peer.instance_name.clone();
    let version = peer.domios_version.clone();
    let instance_id = peer.instance_id.clone();
    let endpoints = mesh
        .peer_route_endpoints(&instance_id)
        .ok_or_else(|| peer_unroutable_error(&name))?;
    Ok((endpoints, instance_id, name, version))
}
