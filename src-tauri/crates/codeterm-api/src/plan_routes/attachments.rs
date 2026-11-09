use axum::extract::{Path as AxumPath, Query, State as AxumState};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::IntoResponse;
use serde::Deserialize;

use crate::routes::AppState;

#[derive(Deserialize)]
pub struct AttachmentQuery {
    path: String,
}

pub async fn attachment(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<AttachmentQuery>,
) -> Result<impl IntoResponse, (StatusCode, &'static str)> {
    tokio::task::spawn_blocking(move || {
        let directory =
            state
                .svc
                .plan_store
                .attachment_directory(&id)
                .map_err(|error| match error {
                    codeterm_service_core::service::plan::PlanError::NotFound => {
                        (StatusCode::NOT_FOUND, "Plan not found")
                    }
                    _ => (StatusCode::INTERNAL_SERVER_ERROR, "Plan read failed"),
                })?;
        if query.path.contains("..") || std::path::Path::new(&query.path).is_absolute() {
            return Err((StatusCode::BAD_REQUEST, "Invalid path"));
        }
        let path = directory.join(&query.path);
        let requested = path
            .canonicalize()
            .map_err(|_| (StatusCode::NOT_FOUND, "File not found"))?;
        let plan_dir = directory
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
    })
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "blocking task failed"))?
}
