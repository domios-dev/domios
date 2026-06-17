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
    // Explicit API request = remote-control command (bridge / agents / tests):
    // broadcast so connected clients flip the pane's view. The in-client
    // toggle stays client-local on purpose (multi-client isolation).
    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "pane-chat-view",
        serde_json::json!({ "pane_id": pane_id, "enabled": body.enabled }),
    );
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

/// Extract a cookie value from the Cookie header.
fn get_cookie<'a>(headers: &'a axum::http::HeaderMap, name: &str) -> Option<&'a str> {
    let cookie_header = headers.get("cookie")?.to_str().ok()?;
    for pair in cookie_header.split(';') {
        let pair = pair.trim();
        if let Some((key, value)) = pair.split_once('=') {
            if key.trim().eq_ignore_ascii_case(name) {
                return Some(value.trim());
            }
        }
    }
    None
}

/// Validate a token against the configured API token or the web token cache.
fn validate_manifest_token(
    state: &AppState,
    candidate: &str,
) -> Result<bool, (StatusCode, &'static str)> {
    if candidate.is_empty() {
        return Ok(false);
    }
    if let Some(ref api_token) = state.config.lock().api.token {
        if !api_token.is_empty()
            && crate::service::tokens::constant_time_eq(api_token.as_bytes(), candidate.as_bytes())
        {
            return Ok(true);
        }
    }
    let cache = state.web_tokens.lock().map_err(|e| {
        log::error!("[web] web token cache lock poisoned: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Web token cache unavailable",
        )
    })?;
    Ok(cache.get(candidate).map_or(false, |created| {
        created.elapsed() < crate::api::routes::WEB_TOKEN_TTL
    }))
}

/// Bootstrap endpoint: validates the bearer token and sets an HttpOnly cookie
/// so the manifest fetch (a browser subresource request) can authenticate
/// without exposing the token in a query string (which leaks via logs/Referer).
pub async fn web_manifest_bootstrap(
    AxumState(state): AxumState<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<
    (
        [(axum::http::header::HeaderName, axum::http::HeaderValue); 1],
        Json<serde_json::Value>,
    ),
    (StatusCode, &'static str),
> {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let token = auth.strip_prefix("Bearer ").unwrap_or("");
    if token.is_empty() || !validate_manifest_token(&state, token)? {
        return Err((StatusCode::UNAUTHORIZED, "Invalid or missing token"));
    }
    let cookie = format!(
        "codeterm-web-token={}; HttpOnly; SameSite=Lax; Path=/web; Max-Age={}",
        token,
        crate::api::routes::WEB_TOKEN_TTL.as_secs()
    );
    Ok((
        [(
            axum::http::header::SET_COOKIE,
            axum::http::HeaderValue::from_str(&cookie).map_err(|e| {
                log::error!("[web] failed to build manifest bootstrap cookie: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to build cookie header",
                )
            })?,
        )],
        Json(serde_json::json!({"ok": true})),
    ))
}

/// Public like the rest of /web. Echoes the token into start_url only when
/// it matches the configured API token, so the installed app launches
/// pre-authenticated; otherwise start_url falls back to the plain mirror.
///
/// Token sources (checked in order):
/// 1. `?token=` query param (legacy — deprecated, leaks via logs/Referer)
/// 2. `codeterm-web-token` cookie (set by web_manifest_bootstrap)
pub async fn web_manifest(
    AxumState(state): AxumState<AppState>,
    headers: axum::http::HeaderMap,
    Query(query): Query<ManifestQuery>,
) -> impl axum::response::IntoResponse {
    // Determine the token: query param (legacy) or cookie (preferred).
    let token: Option<String> = query
        .token
        .clone()
        .or_else(|| get_cookie(&headers, "codeterm-web-token").map(|s| s.to_string()));

    let token_ok = token
        .as_ref()
        .map(|t| validate_manifest_token(&state, t).unwrap_or(false))
        .unwrap_or(false);
    let start_url = if token_ok {
        let t = token.as_deref().unwrap_or_default();
        let encoded: String = url::form_urlencoded::byte_serialize(t.as_bytes()).collect();
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
    /// Mirrors the Tauri command: raw=false means "composed text" — bracket
    /// paste + verified Enter for agent panes. Defaults to true (xterm
    /// keystroke stream).
    #[serde(default = "default_true")]
    raw: bool,
}

fn default_true() -> bool {
    true
}

pub async fn pty_write(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(pty_id): Path<String>,
    Json(body): Json<PtyWriteBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    if body.raw {
        crate::commands::write_pty_string_core(&state.svc, pty_id, body.data)
            .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    } else {
        // Verified injection blocks (drain-checked Enter retries) — keep it
        // off the async runtime.
        let svc = state.svc.clone();
        tokio::task::spawn_blocking(move || {
            let text_stripped = body.data.trim_end_matches('\r');
            let enter = text_stripped.len() != body.data.len();
            svc.send_text_to_pty_smart(&pty_id, text_stripped, enter)
        })
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("send task failed: {e}"),
            )
        })?
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    }
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
    #[serde(default)]
    force: bool,
}

pub async fn claim_resize_owner(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(pane_id): Path<String>,
    Json(body): Json<ResizeOwnerBody>,
) -> Result<Json<crate::resize_owner::ResizeOwner>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let owner = state.resize_owner.lock().claim(
        pane_id,
        body.pty_id,
        body.client_id,
        body.cols,
        body.rows,
        body.force,
    );
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

#[cfg(test)]
mod tests {
    use crate::api::routes::WEB_TOKEN_TTL;

    #[test]
    fn cookie_max_age_uses_web_token_ttl() {
        let ttl = WEB_TOKEN_TTL.as_secs();
        let cookie = format!(
            "codeterm-web-token={}; HttpOnly; SameSite=Lax; Path=/web; Max-Age={}",
            "tok", ttl
        );
        assert!(
            cookie.contains(&format!("Max-Age={ttl}")),
            "Set-Cookie must embed WEB_TOKEN_TTL, got: {cookie}"
        );
    }

    #[test]
    fn constant_time_eq_rejects_last_byte_diff() {
        let token = b"supersecrettoken1234";
        let mut bad = token.to_vec();
        *bad.last_mut().unwrap() ^= 1;
        assert!(
            !crate::service::tokens::constant_time_eq(token, &bad),
            "token differing in last byte must be rejected"
        );
        assert!(
            crate::service::tokens::constant_time_eq(token, token),
            "identical tokens must match"
        );
    }
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

#[derive(serde::Deserialize)]
pub struct TranscribeVoiceBody {
    pub path: String,
}

/// Web-mirror twin of the Tauri `transcribe_voice_attachment` command: format an
/// uploaded voice file into the agent-facing `-=-codeterm:voice ...-=-` line,
/// transcribing when a daemon is configured.
pub async fn transcribe_voice(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<TranscribeVoiceBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let active = crate::config::load_config()
        .integrations
        .voice_transcribe
        .active;
    let line = crate::bridge::voice_transcribe::format_attachment_line(
        &body.path,
        "voice",
        None,
        &state.svc.transcribe_registry,
        &active,
    )
    .await;
    Ok(Json(serde_json::json!({ "line": line })))
}

/// Web-mirror twin of the `plugin_manager_snapshot` Tauri command.
pub async fn plugin_snapshot(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    crate::commands::plugins::snapshot()
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

/// Web-mirror twin of the `plugin_manager_action` Tauri command.
pub async fn plugin_action(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(action): Json<crate::commands::plugins::PluginManagerAction>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    crate::commands::plugins::run_action(action)
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlanceReq {
    pub plugin_id: String,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub ctx: Option<serde_json::Value>,
}

/// Web-mirror twin of the `plugin_glance` Tauri command.
pub async fn plugin_glance(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(req): Json<GlanceReq>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    crate::commands::plugins::glance_render(
        &state.svc.glance_registry,
        &req.plugin_id,
        req.action.as_deref(),
        req.ctx.unwrap_or(serde_json::Value::Null),
    )
    .await
    .map(Json)
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BubbleReq {
    pub plugin_id: String,
    #[serde(default)]
    pub ctx: Option<serde_json::Value>,
}

/// Web-mirror twin of the `plugin_bubble` Tauri command.
pub async fn plugin_bubble(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(req): Json<BubbleReq>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    crate::commands::plugins::bubble_poll(
        &state.svc.bubble_registry,
        &req.plugin_id,
        req.ctx.unwrap_or(serde_json::Value::Null),
    )
    .await
    .map(Json)
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewCallReq {
    pub plugin_id: String,
    pub method: String,
    #[serde(default)]
    pub args: Option<serde_json::Value>,
}

/// Web-mirror twin of the `plugin_view_call` Tauri command (the iframe bridge).
pub async fn plugin_view_call(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(req): Json<ViewCallReq>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    crate::commands::plugins::view_call(
        &state.svc.view_registry,
        &req.plugin_id,
        &req.method,
        req.args.unwrap_or(serde_json::Value::Null),
    )
    .await
    .map(Json)
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

/// The `ct` bridge shim injected (nonce'd) into every plugin iframe. The iframe's
/// ONLY channel to the host: postMessage to the parent, which runs the
/// permission-gated `plugin_view_call`. No direct network (CSP `connect-src 'none'`).
const PLUGIN_BRIDGE_JS: &str = r#"
(function(){var p={},s=0;
window.addEventListener('message',function(e){var m=e.data||{};
 if(m.__ctReply!=null&&p[m.__ctReply]){var d=p[m.__ctReply];delete p[m.__ctReply];
  m.error?d.reject(new Error(m.error)):d.resolve(m.result);}
 if(m.__ctTheme!=null){document.documentElement.style.cssText=m.__ctTheme;}});
window.ct={invoke:function(method,args){var id=++s;return new Promise(function(res,rej){
  p[id]={resolve:res,reject:rej};parent.postMessage({__ctInvoke:id,method:method,args:args||null},'*');});}};
parent.postMessage({__ctReady:true},'*');})();
"#;

#[derive(serde::Deserialize)]
pub struct PluginUiQuery {
    pub token: Option<String>,
}

fn ui_mime(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("woff2") => "font/woff2",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    }
}

/// Serves a plugin's iframe UI from `plugins/<id>/ui/*`, injecting the CSP+nonce
/// and the bridge shim into `index.html`. Static, query-token-gated; the iframe
/// makes no other requests (CSP `connect-src 'none'`) — all power flows through
/// the parent's `plugin_view_call`.
pub async fn plugin_ui(
    AxumState(state): AxumState<AppState>,
    axum::extract::Path((id, path)): axum::extract::Path<(String, String)>,
    Query(q): Query<PluginUiQuery>,
) -> Result<axum::response::Response, (StatusCode, String)> {
    let ok = q
        .token
        .as_deref()
        .map(|t| validate_manifest_token(&state, t).unwrap_or(false))
        .unwrap_or(false);
    if !ok {
        return Err((StatusCode::UNAUTHORIZED, "bad or missing token".into()));
    }
    let dir = crate::view_host::plugin_ui_dir(&id)
        .ok_or((StatusCode::NOT_FOUND, format!("no ui for plugin '{id}'")))?;
    let rel = if path.is_empty() { "index.html" } else { path.as_str() };
    if rel.contains("..") {
        return Err((StatusCode::BAD_REQUEST, "path traversal".into()));
    }
    let file = dir.join(rel);
    let bytes = std::fs::read(&file).map_err(|_| (StatusCode::NOT_FOUND, "not found".to_string()))?;

    use axum::http::{header, HeaderValue};
    let mut resp = if rel.ends_with(".html") {
        let nonce = uuid::Uuid::new_v4().simple().to_string();
        let raw = String::from_utf8_lossy(&bytes).replace("__CT_NONCE__", &nonce);
        let bridge = format!("<script nonce=\"{nonce}\">{PLUGIN_BRIDGE_JS}</script>");
        let html = match raw.find("<head>") {
            Some(i) => format!("{}{}{}", &raw[..i + 6], bridge, &raw[i + 6..]),
            None => format!("{bridge}{raw}"),
        };
        let csp = format!(
            "default-src 'none'; script-src 'nonce-{nonce}'; style-src 'nonce-{nonce}' 'unsafe-inline'; \
             img-src 'self' data:; font-src data:; connect-src 'none'; base-uri 'none'; form-action 'none'"
        );
        let mut r = axum::response::Response::new(axum::body::Body::from(html));
        r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"));
        if let Ok(v) = HeaderValue::from_str(&csp) {
            r.headers_mut().insert(header::CONTENT_SECURITY_POLICY, v);
        }
        r
    } else {
        let mut r = axum::response::Response::new(axum::body::Body::from(bytes));
        r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(ui_mime(rel)));
        r
    };
    resp.headers_mut().insert(
        axum::http::header::X_FRAME_OPTIONS,
        axum::http::HeaderValue::from_static("SAMEORIGIN"),
    );
    Ok(resp)
}

#[derive(Deserialize)]
pub struct SessionTranscriptQuery {
    pub provider: String,
    pub cwd: String,
    pub session_id: String,
}

/// Read-only transcript of a finished run (hidden worker / cron). Renders the
/// same normalized ChatMsg shape as the live chat-ui, parsed straight from the
/// session jsonl — no pane, no tailer, no composer.
pub async fn session_transcript(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<SessionTranscriptQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let (messages, total) = crate::service::pane_chat::session_chat_snapshot(
        &state.svc,
        &q.provider,
        &q.cwd,
        &q.session_id,
        0,
    )
    .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    Ok(Json(
        serde_json::json!({ "messages": messages, "total": total }),
    ))
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
