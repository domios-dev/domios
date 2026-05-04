//! Mesh networking API routes — discovery, pairing, relay, sync.

use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    Json,
};
use tauri::Emitter;

use crate::api::routes::{check_auth, AppState};

pub mod announce;
pub mod connection;
pub mod introduction;
pub mod pairing;
pub mod relay;

pub use announce::{mesh_announce_all, mesh_announce_inbound, mesh_refresh, mesh_set_peer_addr};
pub use connection::{
    mesh_approve_peer, mesh_connect_peer, mesh_disconnect_peer, mesh_reject_peer,
};
pub use introduction::mesh_introduce;
pub use pairing::{
    mesh_pair_accept, mesh_pair_confirm, mesh_pair_reject, mesh_pair_request, mesh_unpair,
};
pub use relay::{mesh_relay, mesh_sync};

pub async fn mesh_ping() -> Json<serde_json::Value> {
    Json(serde_json::json!({"mesh": true}))
}

pub async fn mesh_debug_sign(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let mesh = state.mesh.lock();
    match mesh.sign_request() {
        Ok(token) => Ok(Json(serde_json::json!({"token": token, "instance_id": mesh.ident.identity.instance_id}))),
        Err(e) => Ok(Json(serde_json::json!({"error": e}))),
    }
}

pub async fn mesh_start_broadcast(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let duration = state.config.lock().mesh.broadcast_duration_secs;
    crate::mesh::discovery::start_broadcast(state.mesh.clone(), duration)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to start broadcast"))?;
    let _ = state.svc.app_handle.emit("mesh-state-changed", ());
    Ok(Json(serde_json::json!({"ok": true, "duration_secs": duration})))
}

pub async fn mesh_stop_broadcast(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    crate::mesh::discovery::stop_broadcast(&state.mesh);
    let _ = state.svc.app_handle.emit("mesh-state-changed", ());
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn mesh_status(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let mesh = state.mesh.lock();
    Ok(Json(serde_json::to_value(mesh.status()).unwrap()))
}

pub async fn mesh_peers(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let mesh = state.mesh.lock();
    Ok(Json(serde_json::to_value(mesh.peers_response()).unwrap()))
}

pub async fn mesh_discover(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let mesh = state.mesh.lock();
    let discovered: Vec<_> = mesh.discovered_peers.values().cloned().collect();
    Ok(Json(serde_json::to_value(discovered).unwrap()))
}
