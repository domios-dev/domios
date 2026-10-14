use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};

use crate::api::routes::{check_auth, AppState};
use crate::mem_ops::calibration::*;
use codeterm_mem::curation::calibration::stale::{stale_from_params, StaleParams};

pub async fn mem_calibration_sweep(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<SweepParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let force = p.force.unwrap_or(false);
    let svc = state.svc.clone();
    let result = tokio::task::spawn_blocking(move || {
        svc.mem.with_conn(|conn| run_calibration_sweep(conn, force))
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("join: {}", e)))?
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(result))
}

pub async fn mem_calibration_log(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<CalibLogParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let limit = p.limit.unwrap_or(50).min(500);
    let since = match p.since.as_deref() {
        Some(s) if !s.is_empty() => {
            chrono::DateTime::parse_from_rfc3339(s).map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    format!("since must be RFC3339: {}", e),
                )
            })?;
            s.to_string()
        }
        _ => "1970-01-01T00:00:00Z".to_string(),
    };
    let count_only = p.count.unwrap_or(false);
    let svc = state.svc.clone();
    let result = tokio::task::spawn_blocking(move || {
        svc.mem
            .with_conn(|conn| calibration_log_query(conn, &since, limit, count_only))
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("join: {}", e)))?
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(result))
}

pub async fn mem_calibration_stale(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<StaleParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let svc = state.svc.clone();
    let hygiene = svc.configuration().memory.dream_hygiene;
    tokio::task::spawn_blocking(move || {
        svc.mem
            .with_conn(|conn| stale_from_params(conn, &p, &hygiene))
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("join: {e}")))?
    .map(Json)
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}
