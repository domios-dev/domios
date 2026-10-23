use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use codeterm_service_core::service::remote_worker_transfer::{
    RemoteWorkerTransfer, RemoteWorkerTransferReceipt,
};

use crate::routes::AppState;
use codeterm_service_core::service::remote_team::{
    RemoteFinalizationReceipt, RemoteWorkerFinalize, RemoteWorkerInfo, RemoteWorkerLookup,
};

fn authenticated_full_peer(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<String, (StatusCode, String)> {
    let peer_id = super::relay::check_mesh_relay_auth(state, headers)?;
    let allowed = state
        .mesh
        .lock()
        .paired_peers
        .get(&peer_id)
        .is_some_and(|peer| super::peer_has_full_access(Some(&peer.access_level)));
    if !allowed {
        return Err((
            StatusCode::FORBIDDEN,
            "remote Worker transfer requires Full mesh access".into(),
        ));
    }
    Ok(peer_id)
}

pub async fn mesh_worker_transfer_info(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<RemoteWorkerLookup>,
) -> Result<Json<RemoteWorkerInfo>, (StatusCode, String)> {
    let peer = authenticated_full_peer(&state, &headers)?;
    tokio::task::spawn_blocking(move || {
        state
            .svc
            .inspect_remote_worker(&peer, &request)
            .map(Json)
            .map_err(|error| (StatusCode::CONFLICT, error))
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

pub async fn mesh_worker_finalize(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<RemoteWorkerFinalize>,
) -> Result<Json<RemoteFinalizationReceipt>, (StatusCode, String)> {
    let peer = authenticated_full_peer(&state, &headers)?;
    tokio::task::spawn_blocking(move || {
        state
            .svc
            .finalize_remote_worker(&peer, &request)
            .map(Json)
            .map_err(|error| (StatusCode::CONFLICT, error))
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

fn transfer_for_peer(
    full_access: bool,
    peer_id: &str,
    request: &RemoteWorkerTransfer,
    transfer: impl FnOnce(&str, &RemoteWorkerTransfer) -> Result<RemoteWorkerTransferReceipt, String>,
) -> Result<RemoteWorkerTransferReceipt, (StatusCode, String)> {
    if !full_access {
        return Err((
            StatusCode::FORBIDDEN,
            "remote Worker transfer requires Full mesh access".into(),
        ));
    }
    transfer(peer_id, request).map_err(|error| (StatusCode::CONFLICT, error))
}

pub async fn mesh_worker_transfer(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<RemoteWorkerTransfer>,
) -> Result<Json<RemoteWorkerTransferReceipt>, (StatusCode, String)> {
    let peer_id = super::relay::check_mesh_relay_auth(&state, &headers)?;
    let full_access = state
        .mesh
        .lock()
        .paired_peers
        .get(&peer_id)
        .is_some_and(|peer| super::peer_has_full_access(Some(&peer.access_level)));
    tokio::task::spawn_blocking(move || {
        transfer_for_peer(full_access, &peer_id, &request, |peer, request| {
            state
                .svc
                .transfer_remote_worker_report_target(peer, request)
        })
        .map(Json)
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

#[cfg(test)]
mod tests;
