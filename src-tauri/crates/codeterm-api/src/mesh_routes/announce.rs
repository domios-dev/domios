//! Hot-reload: refresh / announce / set-addr.

use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};

use crate::routes::{check_auth, AppState};

/// Re-detect this instance's local addresses (Tailscale + LAN), then push
/// the new address list to all paired peers via /api/mesh/announce.
///
/// Local origin: standard auth.
/// Remote origin (relayed via /api/mesh/relay): gated to access_level=Full
/// inside `mesh_relay`.
pub async fn mesh_refresh(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;

    codeterm_service_core::mesh::refresh_local_addresses(&state.mesh).await;
    let report = codeterm_service_core::mesh::broadcast_announce(&state.mesh, None).await;

    state
        .svc
        .ui
        .emit("mesh-state-changed", serde_json::Value::Null);
    let mesh = state.mesh.lock();
    Ok(Json(serde_json::json!({
        "ok": true,
        "tailscale_ip": mesh.tailscale_ip,
        "local_ip": mesh.local_ip,
        "transport": mesh.network_transport,
        "web_ui": mesh.local_web_ui,
        "announce": report,
    })))
}

#[derive(serde::Deserialize)]
pub struct AnnounceQuery {
    /// Optional target peer (instance_id or instance_name). Omit to send to all.
    pub to: Option<String>,
}

/// Push our current addresses to peers without re-detecting locally.
pub async fn mesh_announce_all(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<AnnounceQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let report =
        codeterm_service_core::mesh::broadcast_announce(&state.mesh, q.to.as_deref()).await;
    Ok(Json(serde_json::json!({ "ok": true, "report": report })))
}

/// Inbound: a paired peer is telling us its addresses changed.
/// Auth is the same Ed25519 token mesh sync/relay uses; sender_id from
/// the token MUST match `body.instance_id`.
pub async fn mesh_announce_inbound(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(ann): Json<codeterm_service_core::mesh::types::PeerAnnounce>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let token = auth.strip_prefix("Bearer ").unwrap_or("");
    super::require_current_peer_version(&headers)?;
    if !codeterm_service_core::mesh::types::is_supported_relay_protocol(ann.relay_protocol_version)
    {
        return Err((
            StatusCode::UPGRADE_REQUIRED,
            codeterm_service_core::mesh::version::peer_upgrade_message(),
        ));
    }
    let membership = headers
        .get(codeterm_service_core::mesh::account_mesh::protocol::MEMBERSHIP_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let mesh = state.mesh.lock();
    let identity_keys = mesh.paired_identity_keys();
    drop(mesh);

    let sender_id =
        codeterm_service_core::mesh::crown::verify_signed_request(token, &identity_keys)
            .map_err(|e| (StatusCode::UNAUTHORIZED, format!("Unauthorized: {}", e)))?;

    let mut mesh = state.mesh.lock();
    if let Some(membership) = membership.as_deref() {
        mesh.note_advertised_membership(&sender_id, membership);
        mesh.refresh_peer_membership(&sender_id, membership);
    }
    if !mesh.traffic_open(&sender_id) {
        let reason = format!("{:?}", mesh.traffic_decision(&sender_id));
        return Err((StatusCode::FORBIDDEN, reason));
    }
    let adopt =
        membership.is_some() && mesh.is_legacy_link(&sender_id) && mesh.admission_context().is_ok();
    if adopt {
        let mesh_state = state.mesh.clone();
        let peer_id = sender_id.clone();
        codeterm_service_core::async_runtime::spawn(async move {
            if let Err(refusal) =
                codeterm_service_core::mesh::account_mesh::link::adopt_peer(&mesh_state, &peer_id)
                    .await
            {
                log::debug!(
                    "[mesh/account] adoption of {} deferred: {}",
                    peer_id,
                    refusal
                );
            }
        });
    }
    match mesh.apply_announce(&sender_id, &ann) {
        Ok(change) if change.changed() => {
            log::info!(
                "[mesh/announce] sender={} address counts changed from {} to {}",
                sender_id,
                change.previous_addresses.len(),
                ann.addresses.len()
            );
            drop(mesh);
            state
                .svc
                .ui
                .emit("mesh-state-changed", serde_json::Value::Null);
            Ok(Json(serde_json::json!({ "ok": true, "changed": true })))
        }
        Ok(_) => Ok(Json(serde_json::json!({ "ok": true, "changed": false }))),
        Err(e) => Err((StatusCode::FORBIDDEN, e)),
    }
}

#[derive(serde::Deserialize)]
pub struct SetAddrBody {
    pub ip: String,
    #[serde(default)]
    pub port: Option<u16>,
}

/// Manual override: forcibly replace our cached addresses for one paired peer.
/// Local-only — no relay path. Use when auto-detection is wrong.
pub async fn mesh_set_peer_addr(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(peer_id): Path<String>,
    Json(body): Json<SetAddrBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    if body.ip.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "ip is required".to_string()));
    }

    // Resolve by instance_id OR instance_name.
    let resolved_id = {
        let mesh = state.mesh.lock();
        if mesh.paired_peers.contains_key(&peer_id) {
            peer_id.clone()
        } else {
            mesh.paired_peers
                .values()
                .find(|p| p.instance_name == peer_id)
                .map(|p| p.instance_id.clone())
                .ok_or((
                    StatusCode::NOT_FOUND,
                    format!("Peer {} not paired", peer_id),
                ))?
        }
    };

    let mut mesh = state.mesh.lock();
    let prev = mesh
        .set_peer_addr_override(&resolved_id, &body.ip, body.port)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    log::info!(
        "[mesh/set-addr] peer={} address override changed; previous count={}",
        resolved_id,
        prev.len()
    );
    drop(mesh);
    state
        .svc
        .ui
        .emit("mesh-state-changed", serde_json::Value::Null);
    Ok(Json(serde_json::json!({ "ok": true, "previous": prev })))
}
