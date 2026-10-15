use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    Json,
};

use super::{emit_activity, WORKER_ID_HEADER};
use crate::routes::{check_auth, AppState};
use codeterm_service_core::mem_ops::mem_merge::{merge_error_status, merge_items, MergeBody};

pub async fn mem_merge(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(b): Json<MergeBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let worker_id = headers
        .get(WORKER_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(String::from)
        .or_else(|| b.worker_id.clone());
    let ids = vec![b.keeper.clone(), b.lesser.clone()];
    let svc = state.svc.clone();
    let out = tokio::task::spawn_blocking(move || merge_items(&svc, &b, worker_id.as_deref()))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("join: {e}")))?
        .map_err(|e| (merge_error_status(&e), e.into_message()))?;
    emit_activity(&state, &headers, "update", ids);
    Ok(Json(out))
}
