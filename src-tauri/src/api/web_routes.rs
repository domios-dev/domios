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

pub use crate::api::web_plugin_routes::plugin_ui;

/// Full multiplexer snapshot — the web bundle's `mux_get_state`.
pub async fn mux_state(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let snapshot = state.svc.snapshot_with_reconciled_status();
    Ok(Json(serde_json::to_value(snapshot).unwrap_or_default()))
}

pub async fn tab_focus(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((workspace_id, tab_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let tab_id = state
        .svc
        .mux
        .read()
        .snapshot()
        .workspaces
        .into_iter()
        .find(|ws| ws.id == workspace_id)
        .and_then(|ws| ws.tabs.into_iter().find(|tab| tab.id == tab_id))
        .map(|tab| tab.tab_id);
    state
        .svc
        .set_active_tab(&workspace_id, &tab_id)
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    if let Some(tab_id) = tab_id {
        state
            .svc
            .focus_tab(&tab_id)
            .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    }
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

pub async fn tab_chat_view(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(tab_id): Path<String>,
    Json(body): Json<ChatViewBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    state.svc.set_chat_view(&tab_id, body.enabled);
    // Explicit API request = remote-control command (bridge / agents / tests):
    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "tab-chat-view",
        serde_json::json!({ "tab_id": tab_id, "enabled": body.enabled }),
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

pub async fn peer_service_delete(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((instance_id, id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let value = crate::commands_mesh::peer_service_delete_request(&state.mesh, &instance_id, &id)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(value))
}

pub async fn peer_cron_action(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((instance_id, id, action)): Path<(String, String, String)>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let value =
        crate::commands_mesh::peer_cron_action_request(&state.mesh, &instance_id, &id, &action)
            .await
            .map_err(|e| (StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(value))
}

pub async fn peer_cron_history(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((instance_id, id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let value = crate::commands_mesh::peer_cron_history_request(&state.mesh, &instance_id, &id)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(value))
}

/// Optional replay cursor: `?since=<boot_id>:<seq>` (D1).
#[derive(Deserialize)]
pub(crate) struct EventsStreamQuery {
    since: Option<String>,
}

/// Parse a `<boot_id>:<seq>` cursor; the boot_id is dash-free hex (no colon).
fn parse_since(raw: &str) -> Option<(String, u64)> {
    let (boot, seq) = raw.rsplit_once(':')?;
    if boot.is_empty() {
        return None;
    }
    Some((boot.to_string(), seq.parse().ok()?))
}

/// Generic carries a real frontend event name inside — surface that, not "generic".
fn render_event(ev: &crate::api::events::SystemEvent) -> (String, serde_json::Value) {
    use crate::api::events::SystemEvent;
    match ev {
        SystemEvent::Generic { event_type, data } => (event_type.clone(), data.clone()),
        other => (
            other.type_name().to_string(),
            serde_json::to_value(other).unwrap_or(serde_json::Value::Null),
        ),
    }
}

fn event_frame(se: &crate::api::events::SeqEvent, boot: &str) -> Event {
    let (event_type, data) = render_event(&se.event);
    let payload = serde_json::json!({
        "seq": se.seq,
        "boot": boot,
        "event_type": event_type,
        "data": data,
    });
    Event::default().data(payload.to_string())
}

/// The ONLY loss signal (D1): client adopts `next_seq` and full-resyncs.
fn overflow_frame(reason: &str, boot: &str, next_seq: u64, oldest: Option<u64>) -> Event {
    let mut data = serde_json::json!({
        "reason": reason,
        "boot": boot,
        "next_seq": next_seq,
    });
    if let Some(oldest) = oldest {
        data["oldest"] = serde_json::json!(oldest);
    }
    let payload = serde_json::json!({ "event_type": "overflow", "data": data });
    Event::default().data(payload.to_string())
}

/// SSE stream of system events for the web bundle's `listen()` bridge (D1).
/// Frames: `data: {"seq":N,"boot":"..","event_type":"..","data":{..}}`.
/// `?since=<boot>:<seq>` replays missed events from the ring then goes live;
/// overflow/boot-change/lagged emit the explicit overflow frame (no silent gap).
/// Absent `?since` ⇒ start live from now (old-client behavior).
pub async fn events_stream(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<EventsStreamQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, (StatusCode, String)>
{
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;

    use crate::api::events::ReplayPlan;
    // between the snapshot and buffered live events is deduped by seq below.
    let mut rx = state.events.subscribe();
    let snap = state.events.snapshot_ring();
    let boot = snap.boot_id.clone();
    let since = q.since.as_deref().and_then(parse_since);
    let cursor_seq = since.as_ref().map(|(_, s)| *s);
    let plan = snap.plan(since.as_ref().map(|(b, s)| (b.as_str(), *s)));
    let events = state.events.clone();

    let stream = async_stream::stream! {
        // tracks the highest seq yielded so the live loop drops the overlap.
        let (mut replayed_max, replay) = match plan {
            ReplayPlan::FromNow => {
                (0, Vec::new())
            }
            ReplayPlan::BootChanged { next_seq } => {
                yield Ok(overflow_frame("boot-changed", &boot, next_seq, None));
                (0, Vec::new())
            }
            ReplayPlan::Covered { replay } => {
                (cursor_seq.unwrap_or(0), replay)
            }
            ReplayPlan::Evicted { oldest, next_seq, replay } => {
                yield Ok(overflow_frame("ring-evicted", &boot, next_seq, Some(oldest)));
                (next_seq.saturating_sub(1), replay)
            }
        };
        for se in &replay {
            if se.seq > replayed_max {
                replayed_max = se.seq;
            }
            yield Ok(event_frame(se, &boot));
        }
        loop {
            match rx.recv().await {
                Ok(se) => {
                    if se.seq <= replayed_max {
                        continue;
                    }
                    replayed_max = se.seq;
                    yield Ok(event_frame(&se, &boot));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    yield Ok(overflow_frame("lagged", &boot, events.current_seq(), None));
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

/// Validate a token against the configured local API token.
pub(crate) fn validate_manifest_token(
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
    Ok(false)
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
        "codeterm-web-token={}; HttpOnly; SameSite=Lax; Path=/; Max-Age={}",
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

/// Public like the rest of the browser bundle. Echoes the token into start_url only when
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
        format!("/#token={encoded}")
    } else {
        "/".to_string()
    };
    let manifest = serde_json::json!({
        "name": "CodeTerm",
        "short_name": "CodeTerm",
        "id": "/",
        "start_url": start_url,
        "scope": "/",
        "display": "standalone",
        "background_color": "#0d1117",
        "theme_color": "#0d1117",
        "icons": [
            { "src": "/icons/pwa-192.png", "sizes": "192x192", "type": "image/png" },
            { "src": "/icons/pwa-512.png", "sizes": "512x512", "type": "image/png" },
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

#[derive(Deserialize)]
pub(crate) struct SaveTempFileBody {
    data: String,
    extension: String,
    name: Option<String>,
}

/// Web-mirror twin of the `save_temp_file` Tauri command.
pub async fn save_temp_file(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<SaveTempFileBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let path = tokio::task::spawn_blocking(move || {
        crate::commands::save_temp_file(body.data, body.extension, body.name)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({ "path": path })))
}

#[derive(Deserialize)]
pub(crate) struct CopyImageToClipboardBody {
    path: String,
}

/// Web-mirror twin of the `copy_image_to_clipboard` Tauri command. Sets the
/// daemon host's OS clipboard, not the browser's — intentional, since the
/// PTY paste keystroke that follows reads from the same host clipboard.
pub async fn copy_image_to_clipboard(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<CopyImageToClipboardBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    crate::commands::copy_image_to_clipboard(body.path)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[cfg(test)]
mod tests {
    use crate::api::routes::WEB_TOKEN_TTL;
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
        routing::post,
        Router,
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn multipart_upload_contract_accepts_browser_boundary() {
        async fn inspect(mut multipart: axum::extract::Multipart) -> Result<String, StatusCode> {
            let field = multipart
                .next_field()
                .await
                .map_err(|_| StatusCode::BAD_REQUEST)?
                .ok_or(StatusCode::BAD_REQUEST)?;
            let filename = field.file_name().unwrap_or_default().to_string();
            let data = field.bytes().await.map_err(|_| StatusCode::BAD_REQUEST)?;
            Ok(format!("{filename}:{}", data.len()))
        }

        let boundary = "codeterm-browser-boundary";
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"phone photo.png\"\r\nContent-Type: image/png\r\n\r\nimage-bytes\r\n--{boundary}--\r\n"
        );
        let response = Router::new()
            .route("/upload", post(inspect))
            .oneshot(
                Request::post("/upload")
                    .header(
                        "content-type",
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let response = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&response[..], b"phone photo.png:11");
    }

    #[test]
    fn cookie_max_age_uses_web_token_ttl() {
        let ttl = WEB_TOKEN_TTL.as_secs();
        let cookie = format!(
            "codeterm-web-token={}; HttpOnly; SameSite=Lax; Path=/; Max-Age={}",
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
/// recent raw-JSONL window and installs the per-tab tailer whose deltas
/// arrive as `tab-chat-msg-{id}` over the SSE stream.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSubscriptionBody {
    subscription_id: String,
}

pub async fn chat_raw_start(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(tab_id): Path<String>,
    Json(body): Json<ChatSubscriptionBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let svc = state.svc.clone();
    let result = tokio::task::spawn_blocking(move || {
        crate::commands::chat_start_core(&svc, tab_id, body.subscription_id)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
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
    let snapshot = crate::commands::plugins::plugin_manager_snapshot_for_service(state.svc.clone())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(snapshot))
}

/// Web-mirror twin of the `plugin_manager_action` Tauri command.
pub async fn plugin_action(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(action): Json<crate::commands::plugins::PluginManagerAction>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    // keep it off the async runtime (mirrors the Tauri command's spawn_blocking wrap).
    let svc = state.svc.clone();
    let result = tokio::task::spawn_blocking(move || {
        crate::commands::plugins::run_action(action, svc.as_ref())
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    crate::commands::plugins::invalidate_plugin_snapshot();
    Ok(Json(result))
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

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatBackendSessionReq {
    pub plugin_id: String,
    pub session_id: String,
}

/// Web-mirror twin of the `chatbackend_session_info` Tauri command.
pub async fn chatbackend_session_info(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(req): Json<ChatBackendSessionReq>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let svc = state.svc.clone();
    let info = tokio::task::spawn_blocking(move || {
        svc.chatbackend_session_info(&req.plugin_id, &req.session_id)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(info).unwrap_or_default()))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatBackendSwitchModelReq {
    pub plugin_id: String,
    pub session_id: String,
    pub model: String,
}

/// Web-mirror twin of the `chatbackend_switch_model` Tauri command.
pub async fn chatbackend_switch_model(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(req): Json<ChatBackendSwitchModelReq>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let svc = state.svc.clone();
    let info = tokio::task::spawn_blocking(move || {
        svc.chatbackend_set_model(&req.plugin_id, &req.session_id, &req.model)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(info).unwrap_or_default()))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenChatBackendTabReq {
    pub plugin_id: String,
    pub workspace_id: Option<String>,
    pub config: Option<serde_json::Value>,
    pub system_prompt: Option<String>,
    pub preset: Option<String>,
    pub model: Option<String>,
}

/// Web-mirror twin of the `open_chat_backend_tab` Tauri command — opens a
/// non-PTY chatBackend tab (e.g. an LM Studio plugin) from the Extensions panel.
pub async fn open_chat_backend_tab(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(req): Json<OpenChatBackendTabReq>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    use crate::service::spawn::{TabCommand, TabVisibility, SpawnTabSpec};
    let mut config = req.config.unwrap_or(serde_json::Value::Null);
    let overrides = crate::service::spawn::chatbackend_spawn_config(
        req.system_prompt.as_deref(),
        req.preset.as_deref(),
        req.model.as_deref(),
        None,
        None,
        None,
    );
    if let (Some(base), Some(extra)) = (config.as_object_mut(), overrides.as_object()) {
        for (key, value) in extra {
            base.insert(key.clone(), value.clone());
        }
    } else if !overrides.as_object().map(|o| o.is_empty()).unwrap_or(true) {
        config = overrides;
    }
    crate::service::spawn::reject_direct_watcher_chatbackend_config(&config)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let handle = state
        .svc
        .spawn_tab(SpawnTabSpec {
            workspace_id: req.workspace_id,
            visibility: TabVisibility::Visible,
            command: TabCommand::ChatBackend {
                plugin_id: req.plugin_id,
                config,
                role: None,
                role_profile: None,
                can_spawn: None,
                watcher: None,
            },
            ..Default::default()
        })
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({ "tab_id": handle.tab_id })))
}

#[derive(Deserialize)]
pub struct SessionTranscriptQuery {
    pub provider: String,
    pub cwd: String,
    pub session_id: String,
}

/// Read-only transcript of a finished run (hidden worker / cron). Renders the
/// same normalized ChatMsg shape as the live chat-ui, parsed straight from the
/// session jsonl — no tab, no tailer, no composer.
pub async fn session_transcript(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<SessionTranscriptQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let svc = state.svc.clone();
    let (messages, total) = tokio::task::spawn_blocking(move || {
        crate::service::tab_chat::session_chat_snapshot(
            &svc,
            &q.provider,
            &q.cwd,
            &q.session_id,
            0,
        )
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    Ok(Json(
        serde_json::json!({ "messages": messages, "total": total }),
    ))
}

pub async fn chat_raw_stop(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(tab_id): Path<String>,
    Json(body): Json<ChatSubscriptionBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    state
        .svc
        .chat_registry
        .stop(&tab_id, &body.subscription_id);
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
    Path(tab_id): Path<String>,
    Json(body): Json<ChatBackfillBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let svc = state.svc.clone();
    let result = tokio::task::spawn_blocking(move || {
        crate::commands::chat_backfill_core(&svc, tab_id, body.before_offset, body.max_bytes)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|e| (StatusCode::CONFLICT, e))?;
    Ok(Json(serde_json::to_value(result).unwrap_or_default()))
}
