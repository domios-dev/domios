use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use codeterm_types::notification::{validate_answer_labels, CallerAction};
use serde::Deserialize;
use std::io::Read;

use crate::api::routes::{check_auth, AppState};
use crate::notification::reach::Unseen;

const NOTIFICATION_FILE_READ_LIMIT: u64 = 16 * 1024 * 1024;

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
    severity: Option<String>,
    /// Typed at the boundary as the CALLER-facing action set, which has no
    /// invoke variant: a command name has nowhere to land, so it is not a
    /// value some later click handler could interpret.
    actions: Option<Vec<CallerAction>>,
    sticky: Option<bool>,
    tab_id: Option<String>,
    /// Stamped by the Mesh relay from the signed sender; suppresses any tab claim.
    source_peer: Option<String>,
    level: Option<String>,
    event_key: Option<String>,
    /// The surface this notification offers to turn on: `desktop`, or the
    /// service id of a registered push channel. A caller names a surface, never
    /// a config key, and the change still waits for the tap.
    offer: Option<String>,
    /// Which offered label applies the offer. Must be one of this
    /// notification's own choices.
    offer_accept: Option<String>,
    /// The tab whose chat holds the message this notification jumps to.
    target_tab_id: Option<String>,
    /// The exact chat message id inside `target_tab_id`.
    chat_message_id: Option<String>,
}

/// Adopt a caller's offer to be louder. The surface is resolved against the
/// registry and the accepting label against this notification's own choice
/// set, so an offer can neither name a destination that resolves to nothing
/// nor apply on a label no surface will draw.
/// A tap opens the offered column on the row of THIS notification's level, so
/// an offer that names no level could only ever apply nothing. Refused when the
/// offer is made rather than accepted and left silently inert, because an offer
/// that can never apply is a button that lies.
pub(crate) fn offer_requires_level(
    named: &str,
    level: Option<crate::notification::push::NotifyLevel>,
) -> Result<crate::notification::push::NotifyLevel, String> {
    level.ok_or_else(|| {
        format!(
            "Offering '{named}' needs a level: the tap opens that column for the level this notification carries, and one that names no level has no row to open"
        )
    })
}

pub(crate) fn vet_offer(
    svc: &crate::service::ServiceState,
    offer: Option<String>,
    offer_accept: Option<String>,
    actions: Option<&[CallerAction]>,
    level: Option<crate::notification::push::NotifyLevel>,
) -> Result<Option<crate::notification::authority::SurfaceOffer>, String> {
    let Some(named) = offer.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
        if offer_accept.is_some() {
            return Err("An accepting label needs a surface to offer".to_string());
        }
        return Ok(None);
    };
    let accept = offer_accept
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "An offered surface needs the label that accepts it".to_string())?;
    offer_requires_level(named, level)?;
    let target = svc.resolve_surface_target(named)?;
    crate::notification::authority::adopt_offer(target, accept, actions.unwrap_or_default())
        .map(Some)
}

/// An unknown level is rejected here rather than degrading to a runtime
/// string, so an invented level can never be expressed on any transport.
pub(crate) fn parse_push_intent(
    level: Option<&str>,
    event_key: Option<String>,
) -> Result<crate::service::notification::PushIntent, String> {
    let level = match level.map(str::trim).filter(|c| !c.is_empty()) {
        None => None,
        Some(raw) => Some(
            crate::notification::push::NotifyLevel::parse(raw).ok_or_else(|| {
                format!(
                    "Unknown notification level: expected one of {}",
                    crate::notification::push::NotifyLevel::wire_values().join(", ")
                )
            })?,
        ),
    };
    Ok(crate::service::notification::PushIntent { level, event_key })
}

/// Vet a caller-supplied action set. Parsing already closed the kind - and the
/// type it parsed into has no invoke variant at all; this closes the choice
/// set, which only makes sense as a whole.
pub(crate) fn vet_actions(
    actions: Option<Vec<CallerAction>>,
) -> Result<Option<Vec<CallerAction>>, String> {
    if let Some(ref actions) = actions {
        validate_answer_labels(actions)?;
    }
    Ok(actions)
}

pub async fn create_notification(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateNotifBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(code, message)| (code, message.to_string()))?;
    let origin = crate::service::notification::NotificationOrigin {
        caller_tab_id: crate::api::tab_routes::caller_tab_header(&headers),
        claimed_tab_id: body.tab_id,
        peer_name: body.source_peer,
        target: crate::notification::deep_link::NotificationTarget::from_hints(
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
    let (id, unseen) = state.svc.create_notification_ext_reported(
        body.title,
        body.body,
        body.severity.as_deref(),
        actions,
        body.sticky,
        origin,
        intent,
        offer,
    );
    Ok(Json(created_result(&id, unseen)))
}

/// The one shape `notification.create` answers with, on every transport. The
/// id is unconditional — the record was opened — and `unseen` appears only when
/// no surface will show it, so its presence is itself the signal.
pub fn created_result(id: &str, unseen: Option<Unseen>) -> serde_json::Value {
    let mut result = serde_json::json!({ "id": id });
    if let Some(unseen) = unseen {
        result["unseen"] = serde_json::to_value(unseen).unwrap_or_default();
    }
    result
}

/// One row per registered push channel: its service id, its column caption,
/// and whether its own controls may be offered. Rendered once so the Tauri
/// command and the REST route cannot describe the roster differently.
pub fn render_channel_standings(svc: &crate::service::ServiceState) -> serde_json::Value {
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
pub(crate) struct AnswerNotifBody {
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

    /// `notify_unseen` — the result seam every transport answers through: the
    /// id is unconditional, and the report travels only when there is one.
    mod notify_unseen {
        use crate::notification::reach::{ClosedSurface, Unseen, BELL};

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
