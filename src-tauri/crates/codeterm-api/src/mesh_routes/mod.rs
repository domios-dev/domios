//! Mesh networking API routes — account links, relay, sync.

use crate::routes::AppState;
use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    Json,
};

#[cfg(test)]
mod version_tests;

pub mod account_link;
pub mod announce;
pub mod connection;
mod presence;
pub use presence::account_presence;
pub mod relay;
pub(crate) mod relay_stream;
pub mod remote_tabs;
pub mod rpc;
pub mod send_dedupe;
pub mod worker_dispatch;
pub mod worker_observation;
pub mod worker_transfer;
pub use worker_observation::mesh_worker_observation;

pub use account_link::{mesh_link_commit, mesh_link_confirm, mesh_link_hello};
pub use announce::{mesh_announce_all, mesh_announce_inbound, mesh_refresh, mesh_set_peer_addr};
pub use relay::{mesh_relay, mesh_relay_asset, mesh_relay_stream, mesh_relay_to_peer, mesh_sync};
pub use rpc::mesh_rpc;
pub use worker_dispatch::{
    mesh_worker_dispatch, mesh_worker_dispatch_status, mesh_worker_lane_bundle,
};

pub(crate) fn require_current_peer_version(
    headers: &HeaderMap,
) -> Result<(), (StatusCode, String)> {
    let version = headers
        .get(codeterm_service_core::mesh::version::VERSION_HEADER)
        .and_then(|value| value.to_str().ok());
    codeterm_service_core::mesh::version::peer_version_error(version).map_or(Ok(()), |message| {
        Err((StatusCode::UPGRADE_REQUIRED, message))
    })
}

pub async fn mesh_ping(
    AxumState(_state): AxumState<AppState>,
    _headers: HeaderMap,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({"mesh": true}))
}

pub(super) fn require_administrator(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(), (StatusCode, &'static str)> {
    if crate::auth::authenticated_origin(state, headers).is_ok_and(|origin| origin.owner_trusted())
    {
        Ok(())
    } else {
        Err((StatusCode::FORBIDDEN, "Owner authorization required"))
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
    require_administrator(&state, &headers)?;
    let enabled = tokio::task::spawn_blocking(move || {
        codeterm_service_core::mesh::set_mesh_enabled(
            &state.mesh,
            &state.config,
            state.svc.ui.as_ref(),
            body.enabled,
        )
    })
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Mesh config writer task failed",
        )
    })?;
    Ok(Json(serde_json::json!({ "ok": true, "enabled": enabled })))
}

pub async fn mesh_status(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    let mesh = state.mesh.lock();
    Ok(Json(serde_json::to_value(mesh.status()).unwrap()))
}

pub async fn mesh_peers(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    let mesh = state.mesh.lock();
    Ok(Json(serde_json::to_value(mesh.peers_response()).unwrap()))
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
    tokio::task::spawn_blocking(move || {
        require_administrator(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
        let level = match body.access_level.as_str() {
            "full" => codeterm_service_core::mesh::types::AccessLevel::Full,
            "read_only" => codeterm_service_core::mesh::types::AccessLevel::ReadOnly,
            "agent_isolated" => codeterm_service_core::mesh::types::AccessLevel::AgentIsolated,
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
        state.svc.ui.emit(
            "mesh-state-changed",
            serde_json::to_value(serde_json::Value::Null).unwrap_or_default(),
        );
        log::info!(
            "[mesh/api] Set access level for {} to {}",
            instance_id,
            body.access_level
        );
        Ok(Json(serde_json::json!({"ok": true})))
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}
pub(super) fn peer_has_full_access(
    access: Option<&codeterm_service_core::mesh::types::AccessLevel>,
) -> bool {
    access == Some(&codeterm_service_core::mesh::types::AccessLevel::Full)
}
