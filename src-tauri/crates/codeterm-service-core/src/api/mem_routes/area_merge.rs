use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};

use super::{emit_activity, WORKER_ID_HEADER};
use crate::api::routes::{check_auth, AppState};
use crate::mem_ops::area_merge::{merge_area, AreaMergeBody};

pub async fn mem_area_merge(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<AreaMergeBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let worker = headers
        .get(WORKER_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .or_else(|| body.worker_id.clone());
    let svc = state.svc.clone();
    let dry_run = body.dry_run;
    let output = tokio::task::spawn_blocking(move || merge_area(&svc, &body, worker.as_deref()))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    if !dry_run {
        let ids = serde_json::from_value(output["item_ids"].clone()).unwrap_or_default();
        emit_activity(&state, &headers, "update", ids);
    }
    Ok(Json(output))
}
