use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};

use crate::api::routes::{check_auth, AppState};
use crate::service::ServiceContext;
use crate::{CronState, CronWake};

const CRON_WORKER_FAILED: &str = "Cron worker failed";

/// Storage blocks a cron mutation until COMMIT, so it never runs on a runtime worker.
async fn off_runtime<T: Send + 'static>(
    state: &AppState,
    work: impl FnOnce(&ServiceContext, &CronState, &CronWake) -> T + Send + 'static,
) -> Result<T, tokio::task::JoinError> {
    let (svc, cron, wake) = (
        state.svc.clone(),
        state.cron.clone(),
        state.cron_wake.clone(),
    );
    tokio::task::spawn_blocking(move || work(svc.as_ref(), &cron, &wake)).await
}

fn worker_failed(error: tokio::task::JoinError) -> (StatusCode, &'static str) {
    log::error!("[cron] route worker failed: {error}");
    (StatusCode::INTERNAL_SERVER_ERROR, CRON_WORKER_FAILED)
}

fn worker_failed_text(error: tokio::task::JoinError) -> (StatusCode, String) {
    let (status, message) = worker_failed(error);
    (status, message.to_string())
}

pub async fn cron_list(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, error)| (status, error.to_string()))?;
    let ws_id = query.get("workspace_id").map(|s| s.as_str());
    let jobs = state
        .svc
        .cron_list(&state.cron, ws_id)
        .map_err(|error| (StatusCode::SERVICE_UNAVAILABLE, error))?;
    Ok(Json(serde_json::to_value(jobs).unwrap()))
}

pub async fn cron_get(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, error)| (status, error.to_string()))?;
    let job = state
        .svc
        .cron_get(&state.cron, &id)
        .map_err(|error| cron_read_error(error))?;
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
    let job = off_runtime(&state, move |svc, cron, wake| {
        svc.cron_create(cron, wake, body)
    })
    .await
    .map_err(worker_failed_text)?
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
    let job = off_runtime(&state, move |svc, cron, wake| {
        svc.cron_update(cron, wake, &id, body)
    })
    .await
    .map_err(worker_failed_text)?
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
    off_runtime(&state, move |svc, cron, wake| {
        svc.cron_delete(cron, wake, &id)
    })
    .await
    .map_err(worker_failed)?
    .map_err(|_| (StatusCode::NOT_FOUND, "Cron job not found"))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn set_enabled(
    state: AppState,
    headers: HeaderMap,
    id: String,
    enabled: bool,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let job = off_runtime(&state, move |svc, cron, wake| {
        svc.cron_enable(cron, wake, &id, enabled)
    })
    .await
    .map_err(worker_failed)?
    .map_err(|_| (StatusCode::NOT_FOUND, "Cron job not found"))?;
    Ok(Json(serde_json::to_value(job).unwrap()))
}

pub async fn cron_enable(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    set_enabled(state, headers, id, true).await
}

pub async fn cron_disable(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    set_enabled(state, headers, id, false).await
}

pub async fn cron_trigger(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let run = off_runtime(&state, move |svc, cron, _| svc.cron_trigger(cron, &id))
        .await
        .map_err(worker_failed_text)?
        .map_err(|e| {
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
    let cancelled = off_runtime(&state, move |svc, cron, _| {
        svc.cron_cancel_running(cron, &id)
    })
    .await
    .map_err(worker_failed)?
    .map_err(|_| (StatusCode::NOT_FOUND, "Cron job not found"))?;
    Ok(Json(serde_json::json!({"cancelled": cancelled})))
}

pub async fn cron_history(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, error)| (status, error.to_string()))?;
    let history = state
        .svc
        .cron_history(&state.cron, &id)
        .map_err(cron_read_error)?;
    Ok(Json(serde_json::to_value(history).unwrap()))
}

fn cron_read_error(error: String) -> (StatusCode, String) {
    if error.starts_with("cron storage unavailable:") {
        (StatusCode::SERVICE_UNAVAILABLE, error)
    } else {
        (StatusCode::NOT_FOUND, error)
    }
}

#[cfg(test)]
pub(crate) mod off_runtime_gate;
#[cfg(test)]
mod tests;
