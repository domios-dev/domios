use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};

#[derive(Deserialize)]
pub struct SyncParams {
    pub path: Option<String>,
}

pub async fn mem_sync(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<SyncParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let svc = state.svc.clone();
    let result = tokio::task::spawn_blocking(move || {
        if let Some(path) = p.path {
            crate::mem::file_sync::sync_path(&svc.mem, &path)
        } else {
            crate::mem::file_sync::sync_all(&svc.mem)
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
    let conn = state
        .svc
        .mem
        .try_conn_for(std::time::Duration::from_millis(500))
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "mem busy".to_string()))?;
    let total: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM mem_items WHERE source_path IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let stale: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM mem_items WHERE source_path IS NOT NULL AND tags_json LIKE '%source-missing%'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    Ok(Json(serde_json::json!({
        "tracked_files": total,
        "stale_files": stale,
    })))
}
