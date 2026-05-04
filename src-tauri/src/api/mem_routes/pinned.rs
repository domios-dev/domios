use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};

use super::emit_activity;

pub async fn mem_pinned_positions_list(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let positions = state.svc.mem.list_pinned_positions()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "positions": positions })))
}

#[derive(Deserialize)]
pub struct PinnedPositionBody {
    pub x: f64,
    pub y: f64,
}

pub async fn mem_pinned_positions_set(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    axum::extract::Path(mem_id): axum::extract::Path<String>,
    Json(b): Json<PinnedPositionBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let existed = state.svc.mem.set_pinned_position(&mem_id, b.x, b.y)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if !existed {
        return Err((StatusCode::NOT_FOUND, format!("mem_item {} not found", mem_id)));
    }
    emit_activity(&state, &headers, "pin", vec![mem_id]);
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn mem_pinned_positions_delete(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    axum::extract::Path(mem_id): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let removed = state.svc.mem.delete_pinned_position(&mem_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    emit_activity(&state, &headers, "unpin", vec![mem_id]);
    Ok(Json(serde_json::json!({ "removed": removed })))
}
