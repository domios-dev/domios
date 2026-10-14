use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde_json::Value;

use crate::api::mem_routes::{changed_by_from_headers, emit_activity, sanitize_actor};
use crate::api::routes::{check_auth, AppState};
use crate::mem_ops::items::archive::{archive_mem_item, validate_archive_reason, ArchiveBody};

pub async fn mem_archive(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ArchiveBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, error)| (status, error.to_string()))?;
    let reason = validate_archive_reason(&body.reason)?;
    let actor = body
        .changed_by
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(sanitize_actor)
        .unwrap_or_else(|| changed_by_from_headers(&headers));
    let svc = state.svc.clone();
    let target = id.clone();
    let output =
        tokio::task::spawn_blocking(move || archive_mem_item(&svc, &target, &reason, &actor))
            .await
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, format!("join: {error}")))??;
    emit_activity(&state, &headers, "update", vec![id]);
    Ok(Json(output))
}
