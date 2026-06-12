//! REST endpoints for per-pane chat (UI mode).
//!
//! - POST /api/panes/:id/chat/start     → snapshot + spawn tailer
//! - POST /api/panes/:id/chat/stop      → kill tailer
//! - POST /api/panes/:id/chat/backfill  → older messages window

use axum::{
    extract::{Path, Query, State as AxumState},
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

    // Snapshot: parse the trailing window so the UI gets recent context immediately. start_offset = file_size - SNAPSHOT_WINDOW (clamped to 0);
    // the tailer picks up from snapshot.new_offset onwards.
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
    let end = body.before_offset;

    // Read the bounded slice directly so we don't re-ship messages past `before_offset`. parse_session_delta(path, from) reads to EOF — that's the
    // wrong semantics for backfill. Slice + temp file gives us a hard upper bound without changing the trait.
    let bytes = std::fs::read(&path)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "read jsonl failed"))?;
    let total = bytes.len() as u64;
    let clamped_end = end.min(total) as usize;
    let clamped_start = (start as usize).min(clamped_end);
    let window = &bytes[clamped_start..clamped_end];

    // Trim to the last complete line within the window. If there's no \n the window is partial — return empty rather than guess.
    let last_nl = window.iter().rposition(|&b| b == b'\n');
    let messages = if let Some(idx) = last_nl {
        let complete = &window[..=idx];
        // Write bounded content to a temp file so parse_session_delta can parse it via the plugin (trait takes a path, not bytes).
        let tmp_path =
            std::env::temp_dir().join(format!("codeterm-backfill-{}.jsonl", uuid::Uuid::new_v4()));
        std::fs::write(&tmp_path, complete)
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "tempfile write failed"))?;
        let delta = state
            .svc
            .agent_registry
            .with_provider(&info.provider_id, |p| p.parse_session_delta(&tmp_path, 0))
            .unwrap_or(crate::agent::chat_types::SessionDelta {
                new_offset: 0,
                messages: vec![],
            });
        let _ = std::fs::remove_file(&tmp_path);
        delta.messages
    } else {
        vec![]
    };

    Ok(Json(serde_json::json!({
        "ok": true,
        "messages": messages,
        "start_offset": start,
        "end_offset": end,
    })))
}

#[derive(Deserialize)]
pub struct ChatQuery {
    #[serde(default)]
    pub since: usize,
}

/// GET /api/panes/:id/chat?since=N — stateless normalized-chat read (no tailer).
pub async fn snapshot(
    AxumState(state): AxumState<AppState>,
    Path(pane_id): Path<String>,
    Query(q): Query<ChatQuery>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let (messages, total) =
        crate::service::pane_chat::pane_chat_snapshot(&state.svc, &pane_id, q.since)
            .map_err(|_| (StatusCode::BAD_REQUEST, "chat snapshot failed"))?;
    Ok(Json(
        serde_json::json!({ "messages": messages, "count": total }),
    ))
}
