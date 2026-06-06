//! Web-mirror REST surface: HTTP equivalents for Tauri-only IPC commands
//! the browser bundle needs (the bridge in src/lib/bridge.ts maps to
//! these), plus the SSE event stream that replaces Tauri event emits.

use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use futures_core::Stream;
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};

/// Full multiplexer snapshot — the web bundle's `mux_get_state`.
pub async fn mux_state(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let snapshot = state.svc.mux.lock().snapshot();
    Ok(Json(serde_json::to_value(snapshot).unwrap_or_default()))
}

pub async fn tab_focus(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((workspace_id, tab_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    state
        .svc
        .set_active_tab(&workspace_id, &tab_id)
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn tab_close(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((workspace_id, tab_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    state
        .svc
        .close_tab(&workspace_id, &tab_id)
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
pub(crate) struct ChatViewBody {
    enabled: bool,
}

pub async fn pane_chat_view(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(pane_id): Path<String>,
    Json(body): Json<ChatViewBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    state.svc.set_chat_view(&pane_id, body.enabled);
    Ok(Json(serde_json::json!({"ok": true})))
}

/// Peer services for the Mesh tab — same cache+failover logic as the
/// Tauri command (`commands_mesh::fetch_peer_services_view`).
pub async fn peer_services(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(instance_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let view = crate::commands_mesh::fetch_peer_services_view(&state.mesh, &instance_id)
        .await
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    Ok(Json(view))
}

/// SSE stream of system events for the web bundle's `listen()` bridge.
/// Frames are `data: {"event_type": "...", "data": {...}}`.
pub async fn events_stream(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, (StatusCode, String)>
{
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let mut rx = state.events.subscribe();
    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    // Generic carries a real frontend event name inside —
                    // surface that, not the literal "generic".
                    let (event_type, data) = match &ev {
                        crate::api::events::SystemEvent::Generic { event_type, data } => {
                            (event_type.clone(), data.clone())
                        }
                        other => (
                            other.type_name().to_string(),
                            serde_json::to_value(other).unwrap_or(serde_json::Value::Null),
                        ),
                    };
                    let payload = serde_json::json!({
                        "event_type": event_type,
                        "data": data,
                    });
                    yield Ok(Event::default().data(payload.to_string()));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    let payload = serde_json::json!({
                        "event_type": "lagged",
                        "data": { "skipped": n },
                    });
                    yield Ok(Event::default().data(payload.to_string()));
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

// ── PTY over HTTP — the browser terminal's read/write/resize loop ──

/// Long-poll one chunk of PTY output (same semantics as the Tauri
/// `read_pty` the desktop read-loop uses; bytes as a JSON number array).
pub async fn pty_read(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(pty_id): Path<String>,
) -> Result<Json<Vec<u8>>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let data = crate::commands::read_pty_core(&state.svc, pty_id)
        .await
        .map_err(|e| (StatusCode::GONE, e))?;
    Ok(Json(data))
}

#[derive(Deserialize)]
pub(crate) struct PtyWriteBody {
    data: String,
}

pub async fn pty_write(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(pty_id): Path<String>,
    Json(body): Json<PtyWriteBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    crate::commands::write_pty_string_core(&state.svc, pty_id, body.data)
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
pub(crate) struct PtyResizeBody {
    cols: u16,
    rows: u16,
}

pub async fn pty_resize(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(pty_id): Path<String>,
    Json(body): Json<PtyResizeBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    crate::commands::resize_pty_core(&state.svc.pty, pty_id, body.cols, body.rows)
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

// ── chat-raw — the chat view's JSONL window + tailer, web edition ──

/// Same semantics and result shape as the Tauri `chat_start`: returns the
/// recent raw-JSONL window and installs the per-pane tailer whose deltas
/// arrive as `pane-chat-raw-{id}` over the SSE stream.
pub async fn chat_raw_start(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(pane_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let result = crate::commands::chat_start_core(&state.svc, pane_id)
        .map_err(|e| (StatusCode::CONFLICT, e))?;
    Ok(Json(serde_json::to_value(result).unwrap_or_default()))
}

pub async fn chat_raw_stop(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(pane_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    state.svc.chat_registry.stop(&pane_id);
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
pub(crate) struct ChatBackfillBody {
    before_offset: u64,
    #[serde(default)]
    max_bytes: Option<u64>,
}

pub async fn chat_raw_backfill(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(pane_id): Path<String>,
    Json(body): Json<ChatBackfillBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let result =
        crate::commands::chat_backfill_core(&state.svc, pane_id, body.before_offset, body.max_bytes)
            .map_err(|e| (StatusCode::CONFLICT, e))?;
    Ok(Json(serde_json::to_value(result).unwrap_or_default()))
}
