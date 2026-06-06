use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};

#[derive(Debug, Deserialize)]
pub struct LogsQuery {
    #[serde(default)]
    pub level: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub since: Option<i64>,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    500
}

#[derive(Debug, Deserialize)]
pub struct LogsDelete {
    pub before: i64,
}

pub async fn list_logs(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<LogsQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let store = match crate::logs::store::global() {
        Some(s) => s,
        None => return Ok(Json(serde_json::json!([]))),
    };
    let limit = q.limit.clamp(1, 5000);
    let rows = store
        .query(q.level.as_deref(), q.source.as_deref(), q.since, limit)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "logs query failed"))?;
    Ok(Json(
        serde_json::to_value(rows).unwrap_or(serde_json::json!([])),
    ))
}

pub async fn delete_logs(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<LogsDelete>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let store = match crate::logs::store::global() {
        Some(s) => s,
        None => return Ok(Json(serde_json::json!({"deleted": 0}))),
    };
    let n = store
        .delete_before(q.before)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "logs delete failed"))?;
    Ok(Json(serde_json::json!({"deleted": n})))
}
