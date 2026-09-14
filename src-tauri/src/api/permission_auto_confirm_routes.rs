//! Per-tab opt-in auto-confirm of recognized provider permission prompts.

use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, resolve_tab, AppState};

#[derive(Deserialize)]
pub(crate) struct PermissionAutoConfirmBody {
    enabled: bool,
}

pub async fn set_permission_auto_confirm(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
    Json(body): Json<PermissionAutoConfirmBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let tab_id = resolve_tab(&state, &raw_tab_id).map_err(|(s, m)| (s, m.to_string()))?;
    let svc = state.svc.clone();
    let target = tab_id.clone();
    let enabled =
        tokio::task::spawn_blocking(move || svc.set_permission_auto_confirm(&target, body.enabled))
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "permission auto-confirm task join failed".to_string(),
                )
            })?
            .map_err(|error| (StatusCode::NOT_FOUND, error))?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "tab_id": tab_id,
        "enabled": enabled,
    })))
}

pub async fn get_permission_auto_confirm(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let tab_id = resolve_tab(&state, &raw_tab_id).map_err(|(s, m)| (s, m.to_string()))?;
    let (enabled, recent) = state
        .svc
        .permission_auto_confirm_state(&tab_id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("Tab not found: {tab_id}")))?;
    Ok(Json(serde_json::json!({
        "tab_id": tab_id,
        "enabled": enabled,
        "recent": recent,
    })))
}
