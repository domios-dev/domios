//! REST endpoints for per-pane chat (UI mode).
//!
//! - POST /api/panes/:id/chat/start     → snapshot + spawn tailer
//! - POST /api/panes/:id/chat/stop      → kill tailer
//! - POST /api/panes/:id/chat/backfill  → older messages window

use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};
use crate::service::pane_chat::{spawn_tailer, TailConfig};

/// Initial snapshot window — read this many bytes from the tail of
/// the JSONL on `chat/start`. Tuned for typical claude sessions (a
/// long thinking turn + tool result can be 50–100KB; 256KB comfortably
/// holds the recent context without slogging through the whole file).
const SNAPSHOT_WINDOW_BYTES: u64 = 256 * 1024;

/// Default backfill window when client doesn't specify.
const DEFAULT_BACKFILL_BYTES: u64 = 64 * 1024;

#[derive(Deserialize)]
pub struct BackfillBody {
    pub before_offset: u64,
    /// Bytes to read backwards from `before_offset`. Defaults to 64KB.
    #[serde(default)]
    pub max_bytes: Option<u64>,
}

#[derive(serde::Serialize)]
struct ChatPaneInfo {
    provider_id: String,
    cwd: String,
    session_id: String,
}

/// Resolve (provider_id, cwd, session_id) for `pane_id`. Returns the
/// HTTP error tuple ready to bubble up if any required field is
/// missing — keeps the route bodies short.
fn pane_chat_info(
    state: &AppState,
    pane_id: &str,
) -> Result<ChatPaneInfo, (StatusCode, &'static str)> {
    let m = state.svc.mux.lock();
    let pane = m
        .get_pane(pane_id)
        .ok_or((StatusCode::NOT_FOUND, "pane not found"))?;
    let provider_id = pane
        .agent_type
        .clone()
        .ok_or((StatusCode::BAD_REQUEST, "pane has no agent_type"))?;
    let cwd = pane.cwd.clone().unwrap_or_default();
    let session_id = pane
        .agent_session_id
        .clone()
        .ok_or((StatusCode::BAD_REQUEST, "pane has no session_id yet"))?;
    Ok(ChatPaneInfo {
        provider_id,
        cwd,
        session_id,
    })
}

/// Resolve the on-disk JSONL path for `info`, or `NOT_IMPLEMENTED` if
/// the provider plugin returns `None` (e.g. opencode — sqlite-backed).
fn resolve_jsonl_path(
    state: &AppState,
    info: &ChatPaneInfo,
) -> Result<std::path::PathBuf, (StatusCode, &'static str)> {
    state
        .svc
        .agent_registry
        .with_provider(&info.provider_id, |p| {
            p.session_jsonl_path(&info.cwd, &info.session_id)
        })
        .flatten()
        .ok_or((StatusCode::NOT_IMPLEMENTED, "provider has no JSONL"))
}

pub async fn start(
    AxumState(state): AxumState<AppState>,
    Path(pane_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let info = pane_chat_info(&state, &pane_id)?;
    let path = resolve_jsonl_path(&state, &info)?;

    // Snapshot: parse the trailing window so the UI gets recent context
    // immediately. start_offset = file_size - SNAPSHOT_WINDOW (clamped
    // to 0); the tailer picks up from snapshot.new_offset onwards.
    let file_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let start_offset = file_size.saturating_sub(SNAPSHOT_WINDOW_BYTES);
    let snapshot = state
        .svc
        .agent_registry
        .with_provider(&info.provider_id, |p| {
            p.parse_session_delta(&path, start_offset)
        })
        .unwrap_or(crate::agent::chat_types::SessionDelta {
            new_offset: file_size,
            messages: vec![],
        });

    let sink = crate::service::pane_chat::tauri_sink::make_sink(state.svc.app_handle.clone());
    let handle = spawn_tailer(
        TailConfig {
            pane_id: pane_id.clone(),
            provider_id: info.provider_id.clone(),
            cwd: info.cwd.clone(),
            session_id: info.session_id.clone(),
            start_offset: snapshot.new_offset,
            poll_interval: std::time::Duration::from_millis(500),
        },
        state.svc.agent_registry.clone(),
        sink,
    );
    let started = state.svc.chat_registry.insert(pane_id.clone(), handle);
    if !started {
        log::info!(
            "[pane_chat] re-start requested for pane={} (already running); returning fresh snapshot",
            pane_id
        );
    }

    Ok(Json(serde_json::json!({
        "ok": true,
        "snapshot": snapshot.messages,
        "offset": snapshot.new_offset,
    })))
}

pub async fn stop(
    AxumState(state): AxumState<AppState>,
    Path(pane_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    state.svc.chat_registry.stop(&pane_id);
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn backfill(
    AxumState(state): AxumState<AppState>,
    Path(pane_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<BackfillBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let info = pane_chat_info(&state, &pane_id)?;
    let path = resolve_jsonl_path(&state, &info)?;

    let max_bytes = body.max_bytes.unwrap_or(DEFAULT_BACKFILL_BYTES);
    let start = body.before_offset.saturating_sub(max_bytes);
    // We pass `start` to parse_session_delta. The plugin returns
    // every complete line beyond `start`. The frontend filters/trims
    // visually — we don't truncate by end_offset on the server, the
    // parser doesn't have that knob and it's cheap to let the client
    // discard any overlap.
    let delta = state
        .svc
        .agent_registry
        .with_provider(&info.provider_id, |p| {
            p.parse_session_delta(&path, start)
        })
        .unwrap_or(crate::agent::chat_types::SessionDelta {
            new_offset: body.before_offset,
            messages: vec![],
        });

    Ok(Json(serde_json::json!({
        "ok": true,
        "messages": delta.messages,
        "start_offset": start,
        "end_offset": body.before_offset,
    })))
}
