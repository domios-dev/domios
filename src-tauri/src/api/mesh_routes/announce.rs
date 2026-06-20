//! Hot-reload: refresh / announce / set-addr.

use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};

use crate::api::routes::{check_auth, AppState};

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

    crate::mesh::refresh_local_addresses(&state.mesh);
    let report = crate::mesh::broadcast_announce(&state.mesh, None).await;

    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "mesh-state-changed",
        serde_json::Value::Null,
    );
    let mesh = state.mesh.lock();
    Ok(Json(serde_json::json!({
        "ok": true,
        "tailscale_ip": mesh.tailscale_ip,
        "local_ip": mesh.local_ip,
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
    let report = crate::mesh::broadcast_announce(&state.mesh, q.to.as_deref()).await;
    Ok(Json(serde_json::json!({ "ok": true, "report": report })))
}

/// Inbound: a paired peer is telling us its addresses changed.
/// Auth is the same Ed25519 token mesh sync/relay uses; sender_id from
/// the token MUST match `body.instance_id`.
pub async fn mesh_announce_inbound(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(ann): Json<crate::mesh::types::PeerAnnounce>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let token = auth.strip_prefix("Bearer ").unwrap_or("");
    let mesh = state.mesh.lock();
    let mut known_keys = mesh.known_public_keys();
    if let Some(ref pk) = mesh.ident.identity.public_key {
        known_keys.insert(mesh.ident.identity.instance_id.clone(), pk.clone());
    }
    drop(mesh);

    let sender_id = crate::mesh::crown::verify_signed_request(token, &known_keys)
        .map_err(|e| (StatusCode::UNAUTHORIZED, format!("Unauthorized: {}", e)))?;

    let mut mesh = state.mesh.lock();
    match mesh.apply_announce(&sender_id, &ann) {
        Ok(Some(prev)) => {
            log::info!(
                "[mesh/announce] sender={} old={:?} new={:?}",
                sender_id,
                prev.iter().map(|a| &a.ip).collect::<Vec<_>>(),
                ann.addresses.iter().map(|a| &a.ip).collect::<Vec<_>>()
            );
            drop(mesh);
            crate::api::events::emit_everywhere(
                &state.svc.app_handle,
                "mesh-state-changed",
                serde_json::Value::Null,
            );
            Ok(Json(serde_json::json!({ "ok": true, "changed": true })))
        }
        Ok(None) => Ok(Json(serde_json::json!({ "ok": true, "changed": false }))),
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
        "[mesh/set-addr] peer={} override old={:?} new={}{}",
        resolved_id,
        prev.iter().map(|a| &a.ip).collect::<Vec<_>>(),
        body.ip,
        body.port.map(|p| format!(":{}", p)).unwrap_or_default()
    );
    drop(mesh);
    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "mesh-state-changed",
        serde_json::Value::Null,
    );
    Ok(Json(serde_json::json!({ "ok": true, "previous": prev })))
}
