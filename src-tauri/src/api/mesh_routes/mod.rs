//! Mesh networking API routes — discovery, pairing, relay, sync.

use crate::api::routes::{check_auth, AppState};
use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    Json,
};

pub mod announce;
pub mod connection;
pub mod introduction;
pub mod pairing;
pub mod relay;
pub(crate) mod relay_stream;
pub mod remote_tabs;
pub mod rpc;
pub mod send_dedupe;
pub mod worker_dispatch;

pub use announce::{mesh_announce_all, mesh_announce_inbound, mesh_refresh, mesh_set_peer_addr};
pub use connection::{
    mesh_approve_peer, mesh_approve_peer_by_id, mesh_connect_peer, mesh_disconnect_peer,
    mesh_disconnect_peer_by_id, mesh_reject_peer, mesh_reject_peer_by_id,
};
pub use introduction::mesh_introduce;
pub use pairing::{
    mesh_pair_accept, mesh_pair_confirm, mesh_pair_reject, mesh_pair_request, mesh_unpair,
};
pub use relay::{mesh_relay, mesh_relay_asset, mesh_relay_stream, mesh_relay_to_peer, mesh_sync};
pub use rpc::mesh_rpc;
pub use worker_dispatch::mesh_worker_dispatch;

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
        Ok(token) => Ok(Json(
            serde_json::json!({"token": token, "instance_id": mesh.ident.identity.instance_id}),
        )),
        Err(e) => Ok(Json(serde_json::json!({"error": e}))),
    }
}

#[derive(serde::Deserialize)]
pub struct SetEnabledBody {
    pub enabled: bool,
}

/// Manual mesh on/off switch (sidebar toggle, web mirror).
pub async fn mesh_set_enabled(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<SetEnabledBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let enabled = crate::mesh::set_mesh_enabled(
        &state.mesh,
        &state.config,
        &state.svc.app_handle,
        body.enabled,
    );
    Ok(Json(serde_json::json!({ "ok": true, "enabled": enabled })))
}

pub async fn mesh_start_broadcast(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let duration = state.config.lock().mesh.broadcast_duration_secs;
    crate::mesh::discovery::start_broadcast(state.mesh.clone(), duration).map_err(|error| {
        if error == "mesh_listener_not_remotely_ready" {
            (StatusCode::CONFLICT, "mesh_listener_not_remotely_ready")
        } else {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to start broadcast",
            )
        }
    })?;
    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "mesh-state-changed",
        serde_json::Value::Null,
    );
    Ok(Json(
        serde_json::json!({"ok": true, "duration_secs": duration}),
    ))
}

pub async fn mesh_stop_broadcast(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    crate::mesh::discovery::stop_broadcast(&state.mesh);
    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "mesh-state-changed",
        serde_json::Value::Null,
    );
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(serde::Deserialize)]
pub struct AddManualBody {
    pub address: String,
    pub port: u16,
}

pub async fn mesh_add_manual(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<AddManualBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let peer = crate::mesh::types::DiscoveredPeer {
        instance_id: format!("manual-{}", uuid::Uuid::new_v4()),
        instance_name: format!("{}:{}", body.address, body.port),
        addresses: vec![crate::mesh::types::PeerAddress::from_ip(&body.address)],
        api_port: body.port,
        discovery_method: crate::mesh::types::DiscoveryMethod::Manual,
        last_seen: now,
        dev: false,
        mesh_id: None,
        vouched_by: Vec::new(),
    };
    let id = peer.instance_id.clone();
    state.mesh.lock().discovered_peers.insert(id.clone(), peer);
    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "mesh-state-changed",
        serde_json::Value::Null,
    );
    Ok(Json(serde_json::json!({"ok": true, "instance_id": id})))
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

/// Web-mirror twin of the `mesh_exit` Tauri command — notify paired peers
/// then clear all local mesh state.
pub async fn mesh_exit(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let (peers_info, token) = {
        let m = state.mesh.lock();
        let token = m.sign_request().ok();
        let our_id = m.ident.identity.instance_id.clone();
        let peers: Vec<_> = m
            .paired_peers
            .values()
            .map(|p| {
                (
                    p.addresses.first().map(|a| a.ip.clone()),
                    p.api_port,
                    our_id.clone(),
                )
            })
            .collect();
        (peers, token)
    };

    if let Some(token) = &token {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_default();
        for (ip, port, our_id) in &peers_info {
            if let Some(ip) = ip {
                let url = format!("http://{}:{}/api/mesh/peers/{}", ip, port, our_id);
                let _ = http
                    .delete(&url)
                    .header("Authorization", format!("Bearer {}", token))
                    .send()
                    .await;
            }
        }
    }

    state.mesh.lock().exit_mesh();
    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "mesh-state-changed",
        serde_json::Value::Null,
    );
    log::info!("[mesh/api] Exited mesh");
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(serde::Deserialize)]
pub struct SetAccessBody {
    pub access_level: String,
}

/// Web-mirror twin of the `mesh_set_access` Tauri command.
pub async fn mesh_set_access(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    axum::extract::Path(instance_id): axum::extract::Path<String>,
    Json(body): Json<SetAccessBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let level = match body.access_level.as_str() {
        "full" => crate::mesh::types::AccessLevel::Full,
        "read_only" => crate::mesh::types::AccessLevel::ReadOnly,
        "agent_isolated" => crate::mesh::types::AccessLevel::AgentIsolated,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("Invalid access level: {}", body.access_level),
            ))
        }
    };
    {
        let mut m = state.mesh.lock();
        let peer = m
            .paired_peers
            .get_mut(&instance_id)
            .ok_or_else(|| (StatusCode::NOT_FOUND, "Peer not found".to_string()))?;
        peer.access_level = level;
        let _ = m.save();
    }
    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "mesh-state-changed",
        serde_json::Value::Null,
    );
    log::info!(
        "[mesh/api] Set access level for {} to {}",
        instance_id,
        body.access_level
    );
    Ok(Json(serde_json::json!({"ok": true})))
}
