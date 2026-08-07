use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};

#[derive(Deserialize)]
pub(crate) struct ListNotifQuery {
    unread: Option<bool>,
}

pub async fn list_notifications(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListNotifQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let store = state.svc.notif.lock();
    let notifs = store.list(query.unread.unwrap_or(false));
    Ok(Json(serde_json::to_value(notifs).unwrap()))
}

#[derive(Deserialize)]
pub(crate) struct CreateNotifBody {
    title: String,
    body: Option<String>,
    level: Option<String>,
    actions: Option<serde_json::Value>,
    sticky: Option<bool>,
    tab_id: Option<String>,
    /// Stamped by the Mesh relay from the signed sender; suppresses any tab claim.
    source_peer: Option<String>,
}

pub async fn create_notification(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateNotifBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let origin = crate::service::notification::NotificationOrigin {
        caller_tab_id: crate::api::tab_routes::caller_tab_header(&headers),
        claimed_tab_id: body.tab_id,
        peer_name: body.source_peer,
    };
    let id = state.svc.create_notification_ext(
        body.title,
        body.body,
        body.level.as_deref(),
        body.actions,
        body.sticky,
        origin,
    );
    Ok(Json(serde_json::json!({"id": id})))
}

pub async fn clear_notifications(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    state.svc.notif.lock().clear(None);
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn mark_notification_read(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    if !state.svc.notif.lock().mark_read(&id) {
        return Err((StatusCode::NOT_FOUND, "Notification not found"));
    }
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn delete_notification(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    state.svc.notif.lock().clear(Some(&id));
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
pub(crate) struct ServeFileQuery {
    path: String,
}

pub async fn serve_file(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(query): Query<ServeFileQuery>,
) -> Result<axum::response::Response, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;

    // path, so comparing against the plain data_dir() always failed the prefix check (every request 403'd).
    let requested = std::path::Path::new(&query.path)
        .canonicalize()
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid path"))?;
    let data_dir = crate::paths::data_dir();
    let allowed_prefix = data_dir.canonicalize().unwrap_or(data_dir);
    if !requested.starts_with(&allowed_prefix) {
        return Err((StatusCode::FORBIDDEN, "Path not allowed"));
    }

    if !requested.exists() {
        return Err((StatusCode::NOT_FOUND, "File not found"));
    }

    let bytes = std::fs::read(&requested)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to read file"))?;

    let content_type = match requested.extension().and_then(|e| e.to_str()) {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("css") => "text/css",
        Some("js" | "mjs") => "application/javascript",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        _ => "application/octet-stream",
    };

    Ok(axum::response::Response::builder()
        .header("Content-Type", content_type)
        .body(axum::body::Body::from(bytes))
        .unwrap())
}

#[cfg(test)]
mod tests {
    #[test]
    fn serve_file_path_guard_rejects_outside_data_dir() {
        let allowed = crate::paths::data_dir();
        let outside = std::path::Path::new("/etc/passwd");
        let canonical_outside = std::fs::canonicalize(outside).unwrap_or(outside.to_path_buf());
        assert!(
            !canonical_outside.starts_with(&allowed),
            "/etc/passwd should not be inside the data dir"
        );
    }

    #[test]
    fn serve_file_path_guard_accepts_inside_data_dir() {
        let allowed = crate::paths::data_dir();
        let inside = allowed.join("config.toml");
        assert!(inside.starts_with(&allowed));
    }
}
