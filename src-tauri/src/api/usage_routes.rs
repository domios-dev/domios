use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};

#[derive(Debug, Deserialize)]
pub struct UsageQuery {
    pub provider: String,
    #[serde(default)]
    pub account_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UsageHistoryQuery {
    pub provider: String,
    pub since: i64,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub account_id: Option<String>,
}

fn default_limit() -> i64 {
    200
}

#[derive(Debug, Deserialize)]
pub struct UsageRollupQuery {
    pub provider: String,
    pub since: i64,
    #[serde(default)]
    pub account_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UsageRefreshBody {
    pub provider: String,
    #[serde(default)]
    pub account_id: Option<String>,
}

pub async fn usage_latest(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<UsageQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let row = state
        .svc
        .usage_store
        .get_latest(&q.provider, q.account_id.as_deref())
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "usage latest failed"))?;
    Ok(Json(serde_json::to_value(row).unwrap_or(serde_json::json!(null))))
}

pub async fn usage_history(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<UsageHistoryQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let limit = q.limit.clamp(1, 5000);
    let rows = state
        .svc
        .usage_store
        .get_history(&q.provider, q.since, limit, q.account_id.as_deref())
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "usage history failed"))?;
    Ok(Json(serde_json::to_value(rows).unwrap_or(serde_json::json!([]))))
}

pub async fn usage_daily_rollup(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<UsageRollupQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let rows = state
        .svc
        .usage_store
        .get_daily_rollup(&q.provider, q.since)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "usage rollup failed"))?;
    Ok(Json(serde_json::to_value(rows).unwrap_or(serde_json::json!([]))))
}

pub async fn usage_refresh(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<UsageRefreshBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let poller_opt = state.svc.usage_poller.lock().clone();
    let Some(poller) = poller_opt else {
        return Err((StatusCode::SERVICE_UNAVAILABLE, "poller not running"));
    };
    poller.refresh_now(&body.provider, body.account_id);
    let probe_id = format!("probe_{}", ulid::Ulid::new());
    Ok(Json(serde_json::json!({ "queued": true, "probe_id": probe_id })))
}
