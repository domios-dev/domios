use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};

use crate::routes::{check_auth, AppState};
use codeterm_service_core::mem_ops::sync::*;

pub async fn mem_sync(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<SyncParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let svc = state.svc.clone();
    let result = tokio::task::spawn_blocking(move || {
        if let Some(path) = p.path {
            codeterm_service_core::mem::file_sync::sync_path(&svc.mem, &path)
        } else {
            codeterm_service_core::mem::file_sync::sync_all(&svc.mem)
        }
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("join: {}", e)))?
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(&result).unwrap_or_default()))
}

pub async fn mem_sync_status(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let status = state
        .svc
        .mem
        .sync_status(std::time::Duration::from_millis(500))
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "mem busy".to_string()))?;
    Ok(Json(status))
}
