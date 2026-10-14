use axum::extract::{Path as AxumPath, Query, State as AxumState};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::IntoResponse;
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};

#[derive(Deserialize)]
pub struct AttachmentQuery {
    path: String,
}

pub async fn attachment(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<AttachmentQuery>,
) -> Result<impl IntoResponse, (StatusCode, &'static str)> {
    check_auth(&state, &headers).map_err(|_| (StatusCode::UNAUTHORIZED, "Unauthorized"))?;
    let plan = state
        .svc
        .plan_store
        .get(&id)
        .ok_or((StatusCode::NOT_FOUND, "Plan not found"))?;
    let path = state
        .svc
        .plan_store
        .attachment_abs_path(&plan, &query.path)
        .ok_or((StatusCode::BAD_REQUEST, "Invalid path"))?;
    let requested = path
        .canonicalize()
        .map_err(|_| (StatusCode::NOT_FOUND, "File not found"))?;
    let plan_dir = state
        .svc
        .plan_store
        .attachment_abs_path(&plan, "")
        .ok_or((StatusCode::INTERNAL_SERVER_ERROR, "Plan path unavailable"))?
        .canonicalize()
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Plan path unavailable"))?;
    if !requested.starts_with(&plan_dir.join("attachments")) {
        return Err((StatusCode::FORBIDDEN, "Path not allowed"));
    }
    let bytes = std::fs::read(&requested)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Read failed"))?;
    let content_type = match requested.extension().and_then(|ext| ext.to_str()) {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("txt" | "md") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    };
    Ok(([(header::CONTENT_TYPE, content_type)], bytes))
}
