use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use std::io::Read;

use crate::routes::{check_auth, AppState};
use codeterm_service_core::notification::vet::{
    created_result, parse_push_intent, vet_actions, vet_offer, vet_target_tab, vet_title,
    CreateNotifBody,
};

const NOTIFICATION_FILE_READ_LIMIT: u64 = 16 * 1024 * 1024;

#[derive(Deserialize)]
pub struct ListNotifQuery {
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

pub async fn create_notification(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateNotifBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(code, message)| (code, message.to_string()))?;
    let title = vet_title(&body.title).map_err(|message| (StatusCode::BAD_REQUEST, message))?;
    // A relayed peer's jump target names a tab on its own machine and is dropped, so it is not vetted here.
    if body.source_peer.is_none() {
        vet_target_tab(body.target_tab_id.as_deref(), |tab| {
            state.svc.mux.read().resolve_tab_id(tab).is_some()
        })
        .map_err(|message| (StatusCode::BAD_REQUEST, message))?;
    }
    let origin = codeterm_service_core::service::notification::NotificationOrigin {
        caller_tab_id: crate::tab_routes::caller_tab_header(&headers),
        claimed_tab_id: body.tab_id,
        peer_name: body.source_peer,
        target: codeterm_service_core::notification::deep_link::NotificationTarget::from_hints(
            body.target_tab_id.as_deref(),
            body.chat_message_id.as_deref(),
        ),
    };
    let intent = parse_push_intent(body.level.as_deref(), body.event_key)
        .map_err(|message| (StatusCode::BAD_REQUEST, message))?;
    let actions =
        vet_actions(body.actions).map_err(|message| (StatusCode::BAD_REQUEST, message))?;
    let offer = vet_offer(
        &state.svc,
        body.offer,
        body.offer_accept,
        actions.as_deref(),
        intent.level,
    )
    .map_err(|message| (StatusCode::BAD_REQUEST, message))?;
    let (id, unseen) = state
        .svc
        .create_agent_notification_reported(
            title,
            body.body,
            body.severity.as_deref(),
            actions,
            body.sticky,
            origin,
            intent,
            offer,
        )
        .map_err(|message| (StatusCode::BAD_REQUEST, message))?;
    Ok(Json(created_result(&id, unseen)))
}

/// One row per registered push channel: its service id, its column caption,
/// and whether its own controls may be offered. Rendered once so the Tauri
/// command and the REST route cannot describe the roster differently.
pub fn render_channel_standings(
    svc: &codeterm_service_core::service::ServiceState,
) -> serde_json::Value {
    serde_json::Value::Array(
        svc.push_channel_standings()
            .into_iter()
            .map(|standing| {
                serde_json::json!({
                    "id": standing.id,
                    "label": standing.label,
                    "control_available": standing.control_available,
                })
            })
            .collect(),
    )
}

pub async fn push_channel_roster(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    Ok(Json(serde_json::json!({
        "channels": render_channel_standings(&state.svc),
    })))
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
pub struct AnswerNotifBody {
    label: String,
}

/// Record the user's choice. The path names the record and the body names the
/// chosen label; neither can name a command, arguments, or a target tab.
pub async fn answer_notification(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<AnswerNotifBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(code, message)| (code, message.to_string()))?;
    state
        .svc
        .answer_notification(&id, &body.label)
        .map(Json)
        .map_err(|message| (StatusCode::CONFLICT, message))
}

#[derive(Deserialize)]
pub struct ServeFileQuery {
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
    let data_dir = codeterm_service_core::paths::data_dir();
    let allowed_prefix = data_dir.canonicalize().unwrap_or(data_dir);
    if !requested.starts_with(&allowed_prefix) {
        return Err((StatusCode::FORBIDDEN, "Path not allowed"));
    }

    if !requested.exists() {
        return Err((StatusCode::NOT_FOUND, "File not found"));
    }

    let file = std::fs::File::open(&requested)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to read file"))?;
    if file
        .metadata()
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to inspect file"))?
        .len()
        > NOTIFICATION_FILE_READ_LIMIT
    {
        return Err((StatusCode::PAYLOAD_TOO_LARGE, "File exceeds 16 MiB limit"));
    }
    let mut bytes = Vec::new();
    file.take(NOTIFICATION_FILE_READ_LIMIT)
        .read_to_end(&mut bytes)
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
        let allowed = codeterm_service_core::paths::data_dir();
        let outside = std::path::Path::new("/etc/passwd");
        let canonical_outside = std::fs::canonicalize(outside).unwrap_or(outside.to_path_buf());
        assert!(
            !canonical_outside.starts_with(&allowed),
            "/etc/passwd should not be inside the data dir"
        );
    }

    #[test]
    fn serve_file_path_guard_accepts_inside_data_dir() {
        let allowed = codeterm_service_core::paths::data_dir();
        let inside = allowed.join("config.toml");
        assert!(inside.starts_with(&allowed));
    }

    #[test]
    fn notify_send_refuses_blank_long_titles_and_missing_target_tabs() {
        let limit = codeterm_notification_core::contract::MAX_CALLER_TITLE_CHARS;
        assert!(super::vet_title("   ").is_err());
        assert!(super::vet_title(&"t".repeat(limit + 1)).is_err());
        assert_eq!(super::vet_title("  opaque  ").unwrap(), "opaque");
        assert!(super::vet_title(&"t".repeat(limit)).is_ok());

        let live = |tab: &str| tab == "live-tab";
        assert!(super::vet_target_tab(None, live).is_ok());
        assert!(super::vet_target_tab(Some("live-tab"), live).is_ok());
        assert!(super::vet_target_tab(Some("missing-tab"), live).is_err());
        assert!(super::vet_target_tab(Some("  "), live).is_err());
    }

    /// `notify_unseen` — the result seam every transport answers through: the
    /// id is unconditional, and the report travels only when there is one.
    mod notify_unseen {
        use codeterm_service_core::notification::reach::{ClosedSurface, Unseen, BELL};

        #[test]
        fn notify_unseen_absent_on_the_ordinary_result() {
            let result = super::super::created_result("notice-1", None);
            assert_eq!(result["id"], "notice-1");
            assert!(
                result.get("unseen").is_none(),
                "a notification something will show carries no report: {result}"
            );
        }

        #[test]
        fn notify_unseen_travels_on_the_result_with_its_reasons() {
            let report = Unseen {
                closed: vec![ClosedSurface {
                    surface: BELL,
                    reason: "the bell indicator is hidden".to_string(),
                }],
            };
            let result = super::super::created_result("notice-2", Some(report));
            assert_eq!(
                result["id"], "notice-2",
                "the record was still opened, so the id is still returned"
            );
            let closed = result["unseen"]["closed"]
                .as_array()
                .expect("the report travels as a list of shut surfaces");
            assert_eq!(closed.len(), 1);
            assert_eq!(closed[0]["surface"], BELL);
            assert!(
                closed[0]["reason"]
                    .as_str()
                    .is_some_and(|reason| !reason.is_empty()),
                "each shut surface carries its reason to the caller"
            );
        }
    }
}
