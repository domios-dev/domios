//! Web-mirror REST surface: HTTP equivalents for Tauri-only IPC commands
//! the browser bundle needs (the bridge in src/lib/bridge.ts maps to
//! these), plus the SSE event stream that replaces Tauri event emits.

use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use futures_core::Stream;
use serde::Deserialize;
use std::time::{Duration, SystemTime};

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
                    // Generic carries a real frontend event name inside — surface that, not the literal "generic".
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

// ── PWA manifest — served per-token because installed iOS web apps get a storage container separate from Safari ──

#[derive(Deserialize)]
pub(crate) struct ManifestQuery {
    token: Option<String>,
}

/// Public like the rest of /web. Echoes the token into start_url only when
/// it matches the configured API token, so the installed app launches
/// pre-authenticated; otherwise start_url falls back to the plain mirror.
pub async fn web_manifest(
    AxumState(state): AxumState<AppState>,
    Query(query): Query<ManifestQuery>,
) -> impl axum::response::IntoResponse {
    let configured = state.config.lock().api.token.clone();
    let token_ok = matches!(
        (&query.token, &configured),
        (Some(q), Some(c)) if !q.is_empty() && q == c
    );
    let start_url = if token_ok {
        let token = query.token.as_deref().unwrap_or_default();
        let encoded: String = url::form_urlencoded::byte_serialize(token.as_bytes()).collect();
        format!("/web/#token={encoded}")
    } else {
        "/web/".to_string()
    };
    let manifest = serde_json::json!({
        "name": "CodeTerm",
        "short_name": "CodeTerm",
        "id": "/web/",
        "start_url": start_url,
        "scope": "/web/",
        "display": "standalone",
        "background_color": "#0d1117",
        "theme_color": "#0d1117",
        "icons": [
            { "src": "/web/icons/pwa-192.png", "sizes": "192x192", "type": "image/png" },
            { "src": "/web/icons/pwa-512.png", "sizes": "512x512", "type": "image/png" },
        ],
    });
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "application/manifest+json",
        )],
        manifest.to_string(),
    )
}

// ── PTY over HTTP — the browser terminal's read/write/resize loop ──

#[derive(Deserialize)]
pub(crate) struct PtyTailQuery {
    offset: Option<u64>,
}

/// Browser-safe PTY mirror. Unlike the Tauri `read_pty` command, this never
/// reads the PTY master. It tails bytes already captured in OutputBuffer, so
/// web clients cannot steal terminal output from the desktop webview.
pub async fn pty_read(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(pty_id): Path<String>,
    Query(query): Query<PtyTailQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let mut offset = {
        let buf = state.svc.output_buf.lock();
        query.offset.unwrap_or_else(|| buf.total_written(&pty_id))
    };

    let deadline = tokio::time::Instant::now() + Duration::from_secs(25);
    loop {
        // Message avoids "PTY not found"/"PTY closed" — those trigger frontend respawn.
        if state.svc.pty.lock().get_session(&pty_id).is_none() {
            return Err((StatusCode::NOT_FOUND, "pty session ended".to_string()));
        }
        let (data, next_offset) = state.svc.output_buf.lock().read_since(&pty_id, offset);
        offset = next_offset;
        if !data.is_empty() || tokio::time::Instant::now() >= deadline {
            return Ok(Json(serde_json::json!({
                "data": data,
                "offset": offset,
            })));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
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
    client_id: Option<String>,
}

pub async fn pty_resize(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(pty_id): Path<String>,
    Json(body): Json<PtyResizeBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    if let Some(client_id) = body.client_id.as_deref() {
        if !state.resize_owner.lock().is_owner(&pty_id, client_id) {
            return Ok(Json(serde_json::json!({"ok": true, "ignored": true})));
        }
    }
    crate::commands::resize_pty_core(&state.svc.pty, pty_id, body.cols, body.rows)
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
pub(crate) struct ResizeOwnerBody {
    pty_id: String,
    client_id: String,
    cols: u16,
    rows: u16,
}

pub async fn claim_resize_owner(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(pane_id): Path<String>,
    Json(body): Json<ResizeOwnerBody>,
) -> Result<Json<crate::resize_owner::ResizeOwner>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let owner =
        state
            .resize_owner
            .lock()
            .claim(pane_id, body.pty_id, body.client_id, body.cols, body.rows);
    state
        .events
        .publish(crate::api::events::SystemEvent::Generic {
            event_type: "pty-resize-owner-changed".to_string(),
            data: serde_json::to_value(&owner).unwrap_or_default(),
        });
    Ok(Json(owner))
}

// ── uploads — browser attachments are copied onto daemon host for agent CLI path reads.

const UPLOAD_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const UPLOAD_MAX_BYTES: u64 = 512 * 1024 * 1024;
const UPLOAD_TARGET_BYTES: u64 = 384 * 1024 * 1024;

/// Strip anything that could escape the uploads dir or break a shell-quoted
/// paste; keep the extension so the receiving agent can tell what it got.
fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_' => c,
            _ => '_',
        })
        .collect();
    let trimmed = cleaned.trim_matches('.');
    if trimmed.is_empty() {
        "upload.bin".to_string()
    } else {
        trimmed.chars().take(120).collect()
    }
}

struct UploadEntry {
    path: std::path::PathBuf,
    modified: SystemTime,
    size: u64,
}

async fn cleanup_uploads_dir(dir: &std::path::Path) {
    let now = SystemTime::now();
    let Ok(mut rd) = tokio::fs::read_dir(dir).await else {
        return;
    };
    let mut entries: Vec<UploadEntry> = Vec::new();
    let mut total = 0_u64;

    while let Ok(Some(entry)) = rd.next_entry().await {
        let path = entry.path();
        let Ok(meta) = entry.metadata().await else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let size = meta.len();
        if now
            .duration_since(modified)
            .map(|age| age > UPLOAD_TTL)
            .unwrap_or(false)
        {
            let _ = tokio::fs::remove_file(&path).await;
            continue;
        }
        total = total.saturating_add(size);
        entries.push(UploadEntry {
            path,
            modified,
            size,
        });
    }

    if total <= UPLOAD_MAX_BYTES {
        return;
    }
    entries.sort_by_key(|e| e.modified);
    for entry in entries {
        if total <= UPLOAD_TARGET_BYTES {
            break;
        }
        if tokio::fs::remove_file(&entry.path).await.is_ok() {
            total = total.saturating_sub(entry.size);
        }
    }
}

pub async fn upload(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    mut multipart: axum::extract::Multipart,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let dir = crate::paths::data_dir().join("uploads");
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    cleanup_uploads_dir(&dir).await;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    {
        if field.name() != Some("file") {
            continue;
        }
        let name = sanitize_filename(field.file_name().unwrap_or("upload.bin"));
        let data = field
            .bytes()
            .await
            .map_err(|e| (StatusCode::PAYLOAD_TOO_LARGE, e.to_string()))?;
        if data.is_empty() {
            return Err((StatusCode::BAD_REQUEST, "empty file".to_string()));
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        let path = dir.join(format!("{}-{}", &id[..8], name));
        tokio::fs::write(&path, &data)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        return Ok(Json(serde_json::json!({
            "path": path.to_string_lossy(),
            "size": data.len(),
        })));
    }
    Err((StatusCode::BAD_REQUEST, "missing 'file' field".to_string()))
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
    let result = crate::commands::chat_backfill_core(
        &state.svc,
        pane_id,
        body.before_offset,
        body.max_bytes,
    )
    .map_err(|e| (StatusCode::CONFLICT, e))?;
    Ok(Json(serde_json::to_value(result).unwrap_or_default()))
}
