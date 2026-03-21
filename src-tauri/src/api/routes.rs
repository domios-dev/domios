use std::sync::Arc;
use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use base64::Engine as _;
use serde::Deserialize;
use tauri::Emitter;
use tower_http::cors::CorsLayer;

use crate::api::events::{EventBroadcasterState, WebhookRegistration};
use crate::multiplexer::commands::MuxState;
use crate::notification::NotifState;
use crate::screenshot::ScreenshotState;
use crate::{OutputBufferState, PtyState};

#[derive(Clone)]
pub struct AppState {
    pub mux: MuxState,
    pub pty: PtyState,
    pub notif: NotifState,
    pub output_buf: OutputBufferState,
    pub screenshot: ScreenshotState,
    pub events: EventBroadcasterState,
    pub app_handle: tauri::AppHandle,
    pub token: Option<String>,
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/identify", get(identify))
        .route("/api/workspaces", get(list_workspaces).post(create_workspace))
        .route("/api/workspaces/{id}/focus", post(focus_workspace))
        .route("/api/workspaces/{id}/rename", post(rename_workspace))
        .route("/api/workspaces/{id}/close", post(close_workspace))
        .route("/api/workspaces/{id}/tabs", post(create_tab))
        .route("/api/workspaces/{id}/tabs/{tab_id}/move", post(move_tab))
        .route("/api/panes", get(list_panes))
        .route("/api/panes/{id}/split", post(split_pane))
        .route("/api/panes/{id}/send", post(send_text))
        .route("/api/panes/{id}/focus", post(focus_pane))
        .route("/api/panes/{id}/close", post(close_pane))
        .route("/api/panes/{id}/rename", post(rename_pane))
        .route("/api/panes/{id}/output", get(get_pane_output))
        .route("/api/panes/{id}/output/stream", get(stream_pane_output))
        .route("/api/panes/{id}/screen", get(get_pane_screen))
        .route("/api/panes/{id}/exec", post(exec_in_pane))
        .route("/api/panes/{id}/resize", post(resize_pane))
        .route("/api/browser/open", post(open_browser))
        .route("/api/serve-file", get(serve_file))
        .route("/api/notifications", get(list_notifications).post(create_notification))
        .route("/api/notifications/clear", post(clear_notifications))
        .route("/api/webhooks", get(list_webhooks).post(register_webhook))
        .route("/api/webhooks/{id}", axum::routing::delete(unregister_webhook))
        .route("/api/network", get(network_info))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

// Auth check helper
fn check_auth(state: &AppState, headers: &HeaderMap) -> Result<(), (StatusCode, &'static str)> {
    if let Some(ref expected) = state.token {
        let auth = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let token = auth.strip_prefix("Bearer ").unwrap_or("");
        if token != expected {
            return Err((StatusCode::UNAUTHORIZED, "Invalid or missing token"));
        }
    }
    Ok(())
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "name": "CodeTerm"
    }))
}

async fn identify(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    Ok(Json(serde_json::json!({
        "name": "CodeTerm",
        "version": env!("CARGO_PKG_VERSION"),
        "platform": std::env::consts::OS,
        "features": ["multiplexer", "agent-detection", "notifications", "rest-api", "browser"]
    })))
}

async fn list_workspaces(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let mux = state.mux.lock();
    let workspaces = mux.list_workspaces();
    Ok(Json(serde_json::to_value(workspaces).unwrap()))
}

#[derive(Deserialize)]
struct CreateWorkspaceBody {
    name: String,
}

async fn create_workspace(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateWorkspaceBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let ws = {
        let mut mux = state.mux.lock();
        mux.create_workspace(body.name)
    };
    let _ = state.app_handle.emit("mux-state-changed", ());
    Ok(Json(serde_json::to_value(ws).unwrap()))
}

async fn focus_workspace(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    state
        .mux
        .lock()
        .set_active_workspace(&id)
        .map_err(|_| (StatusCode::NOT_FOUND, "Workspace not found"))?;
    let _ = state.app_handle.emit("mux-state-changed", ());
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
struct RenameWorkspaceBody {
    name: String,
}

async fn rename_workspace(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<RenameWorkspaceBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    state
        .mux
        .lock()
        .rename_workspace(&id, &body.name)
        .map_err(|_| (StatusCode::NOT_FOUND, "Workspace not found"))?;
    let _ = state.app_handle.emit("mux-state-changed", ());
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn close_workspace(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pty_ids = state
        .mux
        .lock()
        .close_workspace(&id)
        .map_err(|_| (StatusCode::NOT_FOUND, "Workspace not found"))?;
    for pty_id in &pty_ids {
        let _ = state.pty.lock().kill(pty_id);
    }
    let _ = state.app_handle.emit("mux-state-changed", ());
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
struct MoveTabBody {
    to_workspace_id: String,
}

async fn move_tab(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((workspace_id, tab_id)): Path<(String, String)>,
    Json(body): Json<MoveTabBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    state
        .mux
        .lock()
        .move_tab(&tab_id, &workspace_id, &body.to_workspace_id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Failed to move tab"))?;
    let _ = state.app_handle.emit("mux-state-changed", ());
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn create_tab(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let (tab_id, pane_id) = {
        let mut mux = state.mux.lock();
        mux.create_tab(&workspace_id)
            .map_err(|_| (StatusCode::NOT_FOUND, "Workspace not found"))?
    };
    // Set active tab
    {
        let mut mux = state.mux.lock();
        let _ = mux.set_active_tab(&workspace_id, &tab_id);
    }
    // Tell frontend to refresh — TerminalPane will spawn PTY via Tauri command
    let _ = state.app_handle.emit("mux-state-changed", ());

    Ok(Json(serde_json::json!({
        "tab_id": tab_id,
        "pane_id": pane_id
    })))
}

#[derive(Deserialize)]
struct SplitPaneBody {
    direction: Option<String>,
}

async fn split_pane(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(pane_id): Path<String>,
    body: Option<Json<SplitPaneBody>>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let orientation = match body.as_ref().and_then(|b| b.direction.as_deref()) {
        Some("down" | "vertical") => crate::multiplexer::types::Orientation::Vertical,
        _ => crate::multiplexer::types::Orientation::Horizontal,
    };
    let new_pane_id = {
        let mut mux = state.mux.lock();
        mux.split_pane(&pane_id, orientation)
            .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found"))?
    };
    let _ = state.app_handle.emit("mux-state-changed", ());

    Ok(Json(serde_json::json!({
        "new_pane_id": new_pane_id
    })))
}

async fn list_panes(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let mux = state.mux.lock();
    let snapshot = mux.snapshot();
    let panes: Vec<_> = snapshot.panes.values().collect();
    Ok(Json(serde_json::to_value(panes).unwrap()))
}

#[derive(Deserialize)]
struct SendTextBody {
    text: Option<String>,
    #[serde(default)]
    enter: bool,
    /// Batch: send multiple commands sequentially with optional delays
    batch: Option<Vec<BatchCommand>>,
}

#[derive(Deserialize)]
struct BatchCommand {
    text: String,
    #[serde(default)]
    enter: bool,
    delay_ms: Option<u64>,
}

async fn send_text(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(pane_id): Path<String>,
    Json(body): Json<SendTextBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;

    let pty_id = {
        let mux = state.mux.lock();
        mux.get_pane(&pane_id)
            .and_then(|p| p.pty_id.clone())
            .ok_or((StatusCode::NOT_FOUND, "Pane not found or has no PTY"))?
    };

    // Batch mode — send multiple commands with delays
    if let Some(batch) = body.batch {
        let pty = state.pty.clone();
        let pid = pty_id.clone();
        tokio::spawn(async move {
            for cmd in batch {
                if let Some(delay) = cmd.delay_ms {
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                }
                let text = if cmd.enter && !cmd.text.ends_with('\r') {
                    format!("{}\r", cmd.text)
                } else {
                    cmd.text
                };
                let _ = pty.lock().write(&pid, text.as_bytes());
            }
        });
        return Ok(Json(serde_json::json!({"ok": true, "batch": true})));
    }

    // Single send
    let text = body.text.unwrap_or_default();
    let data = if body.enter && !text.ends_with('\r') {
        format!("{}\r", text)
    } else {
        text
    };

    state
        .pty
        .lock()
        .write(&pty_id, data.as_bytes())
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to write to PTY"))?;

    Ok(Json(serde_json::json!({"ok": true})))
}

async fn focus_pane(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(pane_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    state
        .mux
        .lock()
        .set_focused_pane(&pane_id)
        .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found"))?;
    let _ = state.app_handle.emit("mux-state-changed", ());
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
struct OutputQuery {
    offset: Option<usize>,
    strip_ansi: Option<bool>,
    /// Include current screen text (alternate buffer) in response
    screen: Option<bool>,
}

async fn get_pane_output(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(pane_id): Path<String>,
    Query(query): Query<OutputQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pty_id = {
        let mux = state.mux.lock();
        mux.get_pane(&pane_id)
            .and_then(|p| p.pty_id.clone())
            .ok_or((StatusCode::NOT_FOUND, "Pane not found or has no PTY"))?
    };
    let buf = state.output_buf.lock();
    let text = buf.get_text(&pty_id).unwrap_or_default();
    let offset = query.offset.unwrap_or(0);
    let raw = if offset < text.len() { &text[offset..] } else { "" };
    let output = if query.strip_ansi.unwrap_or(false) {
        strip_ansi_codes(raw)
    } else {
        raw.to_string()
    };
    Ok(Json(serde_json::json!({
        "output": output,
        "offset": text.len(),
    })))
}

#[derive(Deserialize)]
struct ExecBody {
    command: String,
    timeout_ms: Option<u64>,
    strip_ansi: Option<bool>,
}

async fn exec_in_pane(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(pane_id): Path<String>,
    Json(body): Json<ExecBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pty_id = {
        let mux = state.mux.lock();
        mux.get_pane(&pane_id)
            .and_then(|p| p.pty_id.clone())
            .ok_or((StatusCode::NOT_FOUND, "Pane not found or has no PTY"))?
    };

    let start_offset = state.output_buf.lock()
        .get_text(&pty_id).map(|t| t.len()).unwrap_or(0);

    let cmd = if body.command.ends_with('\r') || body.command.ends_with('\n') {
        body.command.clone()
    } else {
        format!("{}\r", body.command)
    };

    state.pty.lock()
        .write(&pty_id, cmd.as_bytes())
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to write to PTY"))?;

    let timeout_ms = body.timeout_ms.unwrap_or(10000);
    let buf_clone = state.output_buf.clone();
    let pty_clone = pty_id.clone();

    // Poll in a spawned task to avoid Send issues
    let result = tokio::spawn(async move {
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_millis(timeout_ms);
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
            let len1 = buf_clone.lock().get_text(&pty_clone).map(|t| t.len()).unwrap_or(0);
            if len1 > start_offset {
                // Wait for output to stabilize (no new data for 800ms)
                tokio::time::sleep(tokio::time::Duration::from_millis(800)).await;
                let len2 = buf_clone.lock().get_text(&pty_clone).map(|t| t.len()).unwrap_or(0);
                if len2 == len1 || tokio::time::Instant::now() >= deadline {
                    let text = buf_clone.lock().get_text(&pty_clone).unwrap_or_default();
                    let new = if start_offset < text.len() { text[start_offset..].to_string() } else { String::new() };
                    return (new, text.len(), false);
                }
            }
            if tokio::time::Instant::now() >= deadline {
                let text = buf_clone.lock().get_text(&pty_clone).unwrap_or_default();
                let new = if start_offset < text.len() { text[start_offset..].to_string() } else { String::new() };
                return (new, text.len(), true);
            }
        }
    }).await.map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Exec task failed"))?;

    let (raw_output, offset, timed_out) = result;
    let output = if body.strip_ansi.unwrap_or(false) {
        strip_ansi_codes(&raw_output)
    } else {
        raw_output
    };
    let mut resp = serde_json::json!({"output": output, "offset": offset});
    if timed_out { resp["timeout"] = serde_json::json!(true); }
    Ok(Json(resp))
}

#[derive(Deserialize)]
struct OpenBrowserBody {
    url: String,
}

async fn open_browser(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<OpenBrowserBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let new_pane_id = {
        let mut mux = state.mux.lock();
        mux.open_browser(Some(body.url.clone()))
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to open browser"))?
    };
    let _ = state.app_handle.emit("mux-state-changed", ());

    Ok(Json(serde_json::json!({
        "pane_id": new_pane_id,
        "url": body.url
    })))
}

async fn close_pane(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(pane_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pty_id = state
        .mux
        .lock()
        .close_pane(&pane_id)
        .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found"))?;
    if let Some(pty_id) = pty_id {
        let _ = state.pty.lock().kill(&pty_id);
    }
    let _ = state.app_handle.emit("mux-state-changed", ());
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
struct RenamePaneBody {
    title: String,
}

async fn rename_pane(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(pane_id): Path<String>,
    Json(body): Json<RenamePaneBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let title = body.title.clone();
    state
        .mux
        .lock()
        .rename_pane(&pane_id, &body.title)
        .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found"))?;
    let _ = state.app_handle.emit("pane-title-pinned", serde_json::json!({
        "pane_id": pane_id,
        "title": title,
    }));
    let _ = state.app_handle.emit("mux-state-changed", ());
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
struct ResizePaneBody {
    cols: u16,
    rows: u16,
}

async fn resize_pane(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(pane_id): Path<String>,
    Json(body): Json<ResizePaneBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pty_id = {
        let mux = state.mux.lock();
        mux.get_pane(&pane_id)
            .and_then(|p| p.pty_id.clone())
            .ok_or((StatusCode::NOT_FOUND, "Pane not found or has no PTY"))?
    };
    state.pty.lock()
        .resize(&pty_id, body.cols, body.rows)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to resize PTY"))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// SSE stream of pane output — real-time output streaming for external agents.
async fn stream_pane_output(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(pane_id): Path<String>,
) -> Result<axum::response::Sse<impl futures_core::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pty_id = {
        let mux = state.mux.lock();
        mux.get_pane(&pane_id)
            .and_then(|p| p.pty_id.clone())
            .ok_or((StatusCode::NOT_FOUND, "Pane not found or has no PTY"))?
    };

    let buf = state.output_buf.clone();
    let pid = pty_id.clone();
    let mut offset = buf.lock().get_text(&pid).map(|t| t.len()).unwrap_or(0);

    let stream = async_stream::stream! {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
            let text = buf.lock().get_text(&pid).unwrap_or_default();
            if text.len() > offset {
                let new_data = text[offset..].to_string();
                offset = text.len();
                yield Ok(axum::response::sse::Event::default()
                    .data(serde_json::json!({"output": new_data, "offset": offset}).to_string()));
            }
        }
    };

    Ok(axum::response::Sse::new(stream)
        .keep_alive(axum::response::sse::KeepAlive::default()))
}

#[derive(Deserialize)]
struct ListNotifQuery {
    unread: Option<bool>,
}

async fn list_notifications(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListNotifQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let store = state.notif.lock();
    let notifs = store.list(query.unread.unwrap_or(false));
    Ok(Json(serde_json::to_value(notifs).unwrap()))
}

#[derive(Deserialize)]
struct CreateNotifBody {
    title: String,
    body: Option<String>,
    level: Option<String>,
}

async fn create_notification(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateNotifBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let level = match body.level.as_deref() {
        Some("warning") => crate::notification::store::NotificationLevel::Warning,
        Some("error") => crate::notification::store::NotificationLevel::Error,
        _ => crate::notification::store::NotificationLevel::Info,
    };
    let level_str = match &level {
        crate::notification::store::NotificationLevel::Warning => "Warning",
        crate::notification::store::NotificationLevel::Error => "Error",
        _ => "Info",
    };
    let title = body.title.clone();
    let notif_body = body.body.clone();
    let id = state
        .notif
        .lock()
        .add(body.title, body.body, level, None, None);
    let _ = state.app_handle.emit("notification-created", serde_json::json!({
        "id": id,
        "title": title,
        "body": notif_body,
        "level": level_str,
        "pane_id": serde_json::Value::Null,
        "workspace_id": serde_json::Value::Null,
    }));

    // Send OS-level notification if window is not focused
    send_os_notification_if_unfocused(&state.app_handle, &title, notif_body.as_deref());

    Ok(Json(serde_json::json!({"id": id})))
}

async fn clear_notifications(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    state.notif.lock().clear(None);
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
struct ScreenshotQuery {
    format: Option<String>,
}

async fn take_screenshot(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(query): Query<ScreenshotQuery>,
) -> Result<axum::response::Response, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    // Screenshot is dev-only — disabled in release builds
    #[cfg(not(debug_assertions))]
    return Err((StatusCode::GONE, "Screenshot API disabled in production"));


    let ss = state.screenshot.clone();
    let app = state.app_handle.clone();

    let result = tokio::task::spawn_blocking(move || {
        crate::screenshot::request_screenshot(&app, &ss, 5000)
    })
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Screenshot task failed"))?
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Screenshot capture failed"))?;

    // result is "data:image/png;base64,..." or raw base64
    let format = query.format.as_deref().unwrap_or("json");

    match format {
        "png" => {
            // Strip data URI prefix if present and decode to raw PNG
            let b64 = result
                .strip_prefix("data:image/png;base64,")
                .unwrap_or(&result);
            let bytes = base64::Engine::decode(
                &base64::engine::general_purpose::STANDARD,
                b64,
            )
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Invalid base64"))?;

            Ok(axum::response::Response::builder()
                .header("Content-Type", "image/png")
                .body(axum::body::Body::from(bytes))
                .unwrap())
        }
        _ => {
            // JSON with base64 data
            let json = serde_json::json!({ "screenshot": result });
            Ok(axum::response::Response::builder()
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(serde_json::to_string(&json).unwrap()))
                .unwrap())
        }
    }
}

#[derive(Deserialize)]
struct ServeFileQuery {
    path: String,
}

async fn serve_file(
    Query(query): Query<ServeFileQuery>,
) -> Result<axum::response::Response, (StatusCode, &'static str)> {
    let path = std::path::Path::new(&query.path);

    if !path.exists() {
        return Err((StatusCode::NOT_FOUND, "File not found"));
    }

    let bytes = std::fs::read(path)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to read file"))?;

    let content_type = match path.extension().and_then(|e| e.to_str()) {
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

// ---------------------------------------------------------------------------
// Screen capture (terminal text content)
// ---------------------------------------------------------------------------

async fn get_pane_screen(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(pane_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    // Verify pane exists
    {
        let mux = state.mux.lock();
        mux.get_pane(&pane_id)
            .ok_or((StatusCode::NOT_FOUND, "Pane not found"))?;
    }

    // Ask frontend to capture screen content via Tauri event
    use tokio::sync::oneshot;
    let (tx, rx) = oneshot::channel::<String>();
    let tx = Arc::new(parking_lot::Mutex::new(Some(tx)));

    let tx_clone = tx.clone();
    let pid = pane_id.clone();

    // Listen for response from frontend
    let app = state.app_handle.clone();
    use tauri::Listener;
    let handler_id = app.listen(format!("screen-capture-response-{}", pid), move |event| {
        if let Some(tx) = tx_clone.lock().take() {
            let payload = event.payload().to_string();
            // Parse the JSON string payload
            let text = serde_json::from_str::<serde_json::Value>(&payload)
                .and_then(|v| Ok(v.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string()))
                .unwrap_or_default();
            let _ = tx.send(text);
        }
    });

    // Request capture from frontend
    use tauri::Emitter;
    let _ = state.app_handle.emit("screen-capture-request", serde_json::json!({
        "pane_id": pane_id,
    }));

    // Wait for response with timeout
    let result = tokio::time::timeout(
        tokio::time::Duration::from_secs(3),
        rx,
    ).await;

    app.unlisten(handler_id);

    match result {
        Ok(Ok(text)) => Ok(Json(serde_json::json!({
            "screen": text,
            "pane_id": pane_id,
        }))),
        _ => Err((StatusCode::REQUEST_TIMEOUT, "Screen capture timed out")),
    }
}

// ---------------------------------------------------------------------------
// Webhooks
// ---------------------------------------------------------------------------

async fn list_webhooks(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let hooks = state.events.list_webhooks();
    Ok(Json(serde_json::to_value(hooks).unwrap()))
}

#[derive(Deserialize)]
struct RegisterWebhookBody {
    url: String,
    #[serde(default)]
    events: Vec<String>,
    #[serde(default)]
    headers: std::collections::HashMap<String, String>,
    pane_id: Option<String>,
    #[serde(default)]
    once: bool,
    payload_extra: Option<serde_json::Value>,
}

async fn register_webhook(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<RegisterWebhookBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let id = uuid::Uuid::new_v4().to_string();
    let reg = WebhookRegistration {
        id: id.clone(),
        url: body.url,
        events: body.events,
        headers: body.headers,
        pane_id: body.pane_id,
        once: body.once,
        payload_extra: body.payload_extra,
    };
    state.events.register_webhook(reg);
    Ok(Json(serde_json::json!({"id": id, "once": body.once})))
}

async fn unregister_webhook(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    state.events.unregister_webhook(&id);
    Ok(Json(serde_json::json!({"ok": true})))
}

// ---------------------------------------------------------------------------
// Network info
// ---------------------------------------------------------------------------

async fn network_info() -> Json<serde_json::Value> {
    let local_ip = local_ip_address().unwrap_or_else(|| "127.0.0.1".to_string());
    let tailscale_ip = detect_tailscale_ip();

    Json(serde_json::json!({
        "local_ip": local_ip,
        "tailscale_ip": tailscale_ip,
        "api_port": 7685,
        "api_url": format!("http://{}:7685", local_ip),
        "tailscale_url": tailscale_ip.as_ref().map(|ip| format!("http://{}:7685", ip)),
    }))
}

pub fn local_ip_address() -> Option<String> {
    use std::net::UdpSocket;
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    socket.local_addr().ok().map(|a| a.ip().to_string())
}

pub fn detect_tailscale_ip() -> Option<String> {
    // Check tailscale status for IP
    let output = std::process::Command::new("tailscale")
        .args(["ip", "-4"])
        .output()
        .ok()?;
    if output.status.success() {
        let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !ip.is_empty() { return Some(ip); }
    }
    None
}

/// Strip ANSI escape codes from terminal output.
pub fn strip_ansi_codes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // ESC sequence — skip until terminator
            if let Some(&next) = chars.peek() {
                if next == '[' {
                    // CSI sequence: ESC [ ... (letter)
                    chars.next();
                    while let Some(&ch) = chars.peek() {
                        chars.next();
                        if ch.is_ascii_alphabetic() || ch == '~' { break; }
                    }
                } else if next == ']' {
                    // OSC sequence: ESC ] ... (BEL or ESC \)
                    chars.next();
                    while let Some(&ch) = chars.peek() {
                        chars.next();
                        if ch == '\x07' { break; }
                        if ch == '\x1b' {
                            if chars.peek() == Some(&'\\') { chars.next(); }
                            break;
                        }
                    }
                } else {
                    chars.next(); // skip one char after ESC
                }
            }
        } else if c == '\r' {
            // Skip \r (carriage return) — keep \n only
            continue;
        } else {
            result.push(c);
        }
    }
    result
}

/// Send an OS-level toast notification if the CodeTerm window is not focused.
pub fn send_os_notification_if_unfocused(app: &tauri::AppHandle, title: &str, body: Option<&str>) {
    use tauri::Manager;
    let is_focused = app
        .get_webview_window("main")
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(true);

    if !is_focused {
        send_os_notification(app, title, body);
    }
}

/// Single notification popup — reuses existing window, auto-closes after 4s.
pub fn send_os_notification(app: &tauri::AppHandle, title: &str, body: Option<&str>) {
    use tauri::Manager;
    use std::sync::atomic::{AtomicBool, Ordering};

    static NOTIF_ACTIVE: AtomicBool = AtomicBool::new(false);
    static LAST_MSG: parking_lot::Mutex<String> = parking_lot::Mutex::new(String::new());

    let msg = format!("{}: {}", title, body.unwrap_or(""));

    // Skip duplicate
    {
        let last = LAST_MSG.lock();
        if *last == msg && NOTIF_ACTIVE.load(Ordering::Relaxed) {
            return;
        }
    }
    *LAST_MSG.lock() = msg.clone();

    let title = title.to_string();
    let body = body.unwrap_or("").to_string();
    let app = app.clone();

    // If notification already showing, update its content
    if NOTIF_ACTIVE.load(Ordering::Relaxed) {
        if let Some(win) = app.get_webview_window("codeterm-notif") {
            let html = build_notif_html(&title, &body);
            let _ = win.eval(&format!(
                "document.open();document.write({});document.close();",
                serde_json::to_string(&html).unwrap_or_default()
            ));
            return;
        }
    }

    NOTIF_ACTIVE.store(true, Ordering::Relaxed);

    std::thread::spawn(move || {
        let (wa_x, wa_y, wa_w, wa_h) = get_workarea();
        let win_w = 320.0;
        let win_h = 80.0;
        let margin = 16.0;

        let html = build_notif_html(&title, &body);

        if let Ok(win) = tauri::WebviewWindowBuilder::new(
            &app,
            "codeterm-notif",
            tauri::WebviewUrl::App("about:blank".into()),
        )
        .title("")
        .inner_size(win_w, win_h)
        .position(wa_x + wa_w - win_w - margin, wa_y + wa_h - win_h - margin)
        .decorations(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .build()
        {
            let _ = win.eval(&format!(
                "document.open();document.write({});document.close();",
                serde_json::to_string(&html).unwrap_or_default()
            ));

            // Auto-close after 4 seconds
            let app_clone = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(4));
                if let Some(w) = app_clone.get_webview_window("codeterm-notif") {
                    let _ = w.close();
                }
                NOTIF_ACTIVE.store(false, Ordering::Relaxed);
            });
        } else {
            NOTIF_ACTIVE.store(false, Ordering::Relaxed);
        }
    });
}

fn build_notif_html(title: &str, body: &str) -> String {
    format!(
        r#"<!DOCTYPE html><html><head><style>
        *{{margin:0;padding:0;box-sizing:border-box}}
        body{{background:#1c2128;color:#c9d1d9;font-family:'Segoe UI',sans-serif;
              padding:12px 16px;border:1px solid #30363d;border-radius:8px;
              overflow:hidden;cursor:pointer}}
        body:hover{{background:#21262d}}
        .t{{font-size:13px;font-weight:600;color:#f0f6fc;margin-bottom:4px}}
        .b{{font-size:12px;color:#8b949e}}
        .i{{float:left;margin-right:10px;font-size:18px;color:#f0883e}}
        </style></head><body>
        <span class="i">&#10035;</span>
        <div class="t">{}</div>
        <div class="b">{}</div>
        </body></html>"#,
        title.replace('<', "&lt;").replace('>', "&gt;"),
        body.replace('<', "&lt;").replace('>', "&gt;"),
    )
}

/// Returns workarea (x, y, width, height) — desktop minus taskbar.
fn get_workarea() -> (f64, f64, f64, f64) {
    #[cfg(target_os = "windows")]
    {
        #[repr(C)]
        struct RECT { left: i32, top: i32, right: i32, bottom: i32 }
        #[link(name = "user32")]
        extern "system" {
            fn SystemParametersInfoW(action: u32, param: u32, pvparam: *mut RECT, winini: u32) -> i32;
        }
        unsafe {
            let mut rect = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
            SystemParametersInfoW(0x0030, 0, &mut rect, 0);
            // Divide by DPI scale — Tauri uses logical pixels
            let dpi = {
                #[link(name = "user32")]
                extern "system" { fn GetDpiForSystem() -> u32; }
                unsafe { GetDpiForSystem() }
            };
            let scale = dpi as f64 / 96.0;
            let x = rect.left as f64 / scale;
            let y = rect.top as f64 / scale;
            let w = (rect.right - rect.left) as f64 / scale;
            let h = (rect.bottom - rect.top) as f64 / scale;
            (x, y, w, h)
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        (0.0, 0.0, 1920.0, 1080.0)
    }
}
