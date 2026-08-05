use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};

#[derive(Deserialize)]
pub(crate) struct DoneQuery {
    limit: Option<usize>,
}

/// Durable Done surface: completed lanes across all orchestrators (done /
/// in_review), most-recently-finished first. Backs the UI Done view so a pane
/// that finished and self-closed stays inspectable with its status, timestamp,
/// commit SHA, and report message.
pub async fn list_done(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(query): Query<DoneQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let limit = query.limit.unwrap_or(200).min(1000);
    let rows = state.svc.report_store.lock().completed_rows(limit);
    Ok(Json(serde_json::json!({ "completed": rows })))
}
