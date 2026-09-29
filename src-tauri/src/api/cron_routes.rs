use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};

use crate::api::routes::{check_auth, AppState};

pub async fn cron_list(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let ws_id = query.get("workspace_id").map(|s| s.as_str());
    let jobs = state.svc.cron_list(&state.cron, ws_id);
    Ok(Json(serde_json::to_value(jobs).unwrap()))
}

pub async fn cron_get(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let job = state
        .svc
        .cron_get(&state.cron, &id)
        .map_err(|_| (StatusCode::NOT_FOUND, "Cron job not found"))?;
    Ok(Json(serde_json::to_value(job).unwrap()))
}

pub async fn cron_create(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(mut body): Json<crate::cron::store::CronCreateParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, error)| (status, error.to_string()))?;
    if body.creator_tab_id.is_none() {
        body.creator_tab_id = crate::api::tab_routes::caller_tab_header(&headers);
    }
    let job = state
        .svc
        .cron_create(&state.cron, &state.cron_wake, body)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    Ok(Json(serde_json::to_value(job).unwrap()))
}

pub async fn cron_update(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<crate::cron::store::CronUpdateParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, error)| (status, error.to_string()))?;
    let job = state
        .svc
        .cron_update(&state.cron, &state.cron_wake, &id, body)
        .map_err(|error| {
            let status = if error.starts_with("Cron job not found:") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::BAD_REQUEST
            };
            (status, error)
        })?;
    Ok(Json(serde_json::to_value(job).unwrap()))
}

pub async fn cron_delete(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    state
        .svc
        .cron_delete(&state.cron, &state.cron_wake, &id)
        .map_err(|_| (StatusCode::NOT_FOUND, "Cron job not found"))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn cron_enable(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let job = state
        .svc
        .cron_enable(&state.cron, &state.cron_wake, &id, true)
        .map_err(|_| (StatusCode::NOT_FOUND, "Cron job not found"))?;
    Ok(Json(serde_json::to_value(job).unwrap()))
}

pub async fn cron_disable(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let job = state
        .svc
        .cron_enable(&state.cron, &state.cron_wake, &id, false)
        .map_err(|_| (StatusCode::NOT_FOUND, "Cron job not found"))?;
    Ok(Json(serde_json::to_value(job).unwrap()))
}

pub async fn cron_trigger(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let run = state.svc.cron_trigger(&state.cron, &id).map_err(|e| {
        let code = if e.contains("already running") {
            StatusCode::CONFLICT
        } else if e.contains("not found") {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };
        (code, e)
    })?;
    Ok(Json(serde_json::to_value(run).unwrap()))
}

/// Stop any currently-running executions of a cron job without deleting
/// the job definition. Future scheduled fires continue normally.
/// Returns `{cancelled: N}` where N is the number of runs stopped.
pub async fn cron_cancel(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let cancelled = state
        .svc
        .cron_cancel_running(&state.cron, &id)
        .map_err(|_| (StatusCode::NOT_FOUND, "Cron job not found"))?;
    Ok(Json(serde_json::json!({"cancelled": cancelled})))
}

pub async fn cron_history(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    state
        .svc
        .cron_get(&state.cron, &id)
        .map_err(|_| (StatusCode::NOT_FOUND, "Cron job not found"))?;
    let history = state.svc.cron_history(&state.cron, &id);
    Ok(Json(serde_json::to_value(history).unwrap()))
}
