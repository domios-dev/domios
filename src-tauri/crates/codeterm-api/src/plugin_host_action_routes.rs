//! `ct.host.*` user-gesture actions of a plugin view. The app stamps the plugin
//! id and verifies the gesture; these routes enforce the declaration and media
//! ownership (see `codeterm_commands::commands::plugins::view_host_action`).

use axum::{
    extract::{Request, State as AxumState},
    handler::Handler,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{post, MethodRouter},
    Json,
};
use futures_util::StreamExt;
use serde::Deserialize;

use crate::auth::CallerIdentity;
use crate::routes::{check_auth, AppState};
use codeterm_commands::commands::plugins::{
    view_host_action, view_host_download, view_host_input, DocumentStamp, HostActionRequest,
    PickedSource,
};

const DOWNLOAD_CHUNK: usize = 64 * 1024;
/// The private document lease the app stamps on every host-action request; never the public
/// document id or the presentation key, and never anything the frame supplies.
const VIEW_LEASE_HEADER: &str = "x-domios-plugin-view-frame";
/// The document's non-secret registration key, also stamped by the app's loader; the daemon only
/// accepts it when the private lease resolves to that same registration.
const REGISTRATION_HEADER: &str = "x-domios-plugin-media-frame";
const INPUT_NAME_HEADER: &str = "x-domios-plugin-input-name";
const INPUT_PLUGIN_HEADER: &str = "x-domios-plugin-id";
/// Bounds on the web mirror's percent-encoded file name: bytes on the wire, characters decoded.
const MAX_ENCODED_NAME_BYTES: usize = 360;
const MAX_NAME_CHARS: usize = 120;

/// The document stamp as owned header values, so it can cross into a blocking task.
#[derive(Clone, Default)]
struct Stamp {
    lease: Option<String>,
    registration: Option<String>,
}

impl Stamp {
    fn of(headers: &HeaderMap) -> Self {
        let value = |name: &str| headers.get(name).and_then(|value| value.to_str().ok()).map(str::to_owned);
        Self {
            lease: value(VIEW_LEASE_HEADER),
            registration: value(REGISTRATION_HEADER),
        }
    }

    fn borrow(&self) -> DocumentStamp<'_> {
        DocumentStamp {
            lease: self.lease.as_deref(),
            registration: self.registration.as_deref(),
        }
    }
}

/// Only the local owner (the desktop app, or the owner's own web session) may act for a view.
/// A tab, peer or service credential could otherwise name any plugin, frame, handle and
/// destination itself and skip the app's gesture and save dialog.
pub async fn require_local_owner(request: Request, next: Next) -> Response {
    if matches!(
        request.extensions().get::<CallerIdentity>(),
        Some(CallerIdentity::Local)
    ) {
        next.run(request).await
    } else {
        (
            StatusCode::FORBIDDEN,
            "Plugin view host actions are local-owner only",
        )
            .into_response()
    }
}

/// `POST` behind [`require_local_owner`], so a refused caller never reaches the handler.
pub fn owner_post<H, T, S>(handler: H) -> MethodRouter<S>
where
    H: Handler<T, S>,
    T: 'static,
    S: Clone + Send + Sync + 'static,
{
    post(handler).route_layer(middleware::from_fn(require_local_owner))
}

/// Refusals are typed results, so they share the success status.
pub async fn host_action(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(request): Json<HostActionRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let svc = state.svc.clone();
    let stamp = Stamp::of(&headers);
    tokio::task::spawn_blocking(move || view_host_action(&svc, request, stamp.borrow()))
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DownloadRequest {
    plugin_id: String,
    #[serde(default)]
    handle: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DialogInput {
    plugin_id: String,
    /// What the app's native open dialog returned; never a frame value. Trust assumption, stated
    /// plainly: the daemon cannot prove a path came from a dialog. It accepts one only because this
    /// route is local-owner-only (agents, peers and services are refused before the handler) and
    /// the app builds it solely from `dialogs.open`; the frame can set neither the route, the
    /// plugin, the lease nor the path. Any local-owner caller could ingest a file it can read.
    path: String,
    file_name: String,
}

/// Decode the web mirror's percent-encoded UTF-8 file name once, within its byte and character bounds.
fn input_name(headers: &HeaderMap) -> Option<String> {
    let encoded = headers.get(INPUT_NAME_HEADER)?.to_str().ok()?;
    if encoded.len() > MAX_ENCODED_NAME_BYTES {
        return None;
    }
    let mut bytes = Vec::with_capacity(encoded.len());
    let mut raw = encoded.bytes();
    while let Some(byte) = raw.next() {
        if byte == b'%' {
            let pair = [raw.next()?, raw.next()?];
            bytes.push(u8::from_str_radix(std::str::from_utf8(&pair).ok()?, 16).ok()?);
        } else {
            bytes.push(byte);
        }
    }
    let name = String::from_utf8(bytes).ok()?;
    (name.chars().count() <= MAX_NAME_CHARS).then_some(name)
}

/// Opens the dialog's choice as the final component of its (resolved) folder without following a
/// link at the moment of opening, and only when it is a regular file.
fn open_dialog_path(raw: &str) -> Result<std::fs::File, serde_json::Value> {
    let refused = |code: &str, reason: &str| serde_json::json!({ "ok": false, "code": code, "reason": reason });
    let path = std::path::Path::new(raw);
    if !path.is_absolute() {
        return Err(refused("invalid", "path"));
    }
    let resolved = std::fs::canonicalize(path).map_err(|_| refused("unavailable", "unreadable"))?;
    let (Some(folder), Some(name)) = (resolved.parent(), resolved.file_name()) else {
        return Err(refused("invalid", "path"));
    };
    let file = codeterm_blueprint_types::code_source::open_file(folder, &name.to_string_lossy())
        .map_err(|_| refused("unavailable", "unreadable"))?;
    match file.metadata() {
        Ok(meta) if meta.is_file() => Ok(file),
        _ => Err(refused("unavailable", "unreadable")),
    }
}

/// `pickFile` ingestion. Desktop posts JSON `{pluginId, path, fileName}` with the dialog's choice;
/// the web mirror posts the file itself as the body with its type, a percent-encoded name and the
/// plugin id in headers. Both answer a typed result with the new frame handle, never a path.
pub async fn host_input(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    body: axum::body::Body,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let svc = state.svc.clone();
    let stamp = Stamp::of(&headers);
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.split(';').next().unwrap_or_default().trim().to_ascii_lowercase())
        .unwrap_or_default();
    let invalid = |reason: &str| Json(serde_json::json!({ "ok": false, "code": "invalid", "reason": reason }));
    if content_type == "application/json" {
        let bytes = axum::body::to_bytes(body, 8 * 1024)
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
        let Ok(input) = serde_json::from_slice::<DialogInput>(&bytes) else {
            return Ok(invalid("request"));
        };
        return tokio::task::spawn_blocking(move || {
            let file = match open_dialog_path(&input.path) {
                Ok(file) => file,
                Err(refusal) => return refusal,
            };
            view_host_input(&svc, &input.plugin_id, stamp.borrow(), PickedSource::Opened(file), &input.file_name, "")
        })
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
    }
    let Some(name) = input_name(&headers) else {
        return Ok(invalid("name"));
    };
    let Some(plugin_id) = headers
        .get(INPUT_PLUGIN_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
    else {
        return Ok(invalid("request"));
    };
    // The body streams into ingestion, which enforces the declared size and type. The wait for each
    // chunk is bounded here, by the absolute ingestion deadline and by the asking document staying
    // live, so a stalled upload never pins the blocking thread, the partial file or the frame.
    let stream = body
        .into_data_stream()
        .map(|chunk| chunk.map_err(std::io::Error::other));
    let live = {
        let (plugin_id, lease) = (plugin_id.clone(), stamp.lease.clone());
        move || {
            lease.as_deref().is_some_and(|lease| {
                codeterm_plugin::media_delivery::view_frame(&plugin_id, lease).is_ok()
            })
        }
    };
    let reader = BodyReader::new(
        stream,
        tokio::runtime::Handle::current(),
        tokio::time::Instant::now() + INGEST_DEADLINE,
        LIVENESS_SLICE,
        live,
    );
    // A type the browser could not name is no hint at all: ingestion then sniffs the content.
    let hint = if content_type == "application/octet-stream" { String::new() } else { content_type };
    tokio::task::spawn_blocking(move || {
        view_host_input(&svc, &plugin_id, stamp.borrow(), PickedSource::Body(Box::new(reader)), &name, &hint)
    })
    .await
    .map(Json)
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

/// The whole web upload must finish within this, matching media delivery's ingestion deadline.
const INGEST_DEADLINE: std::time::Duration = std::time::Duration::from_secs(120);
/// How long one wait for the next chunk may last before the reader rechecks that the asking
/// document is still live; a retired or reminted lease ends the upload within this.
const LIVENESS_SLICE: std::time::Duration = std::time::Duration::from_secs(1);

/// A blocking reader over the request body for ingestion on a blocking thread. Every wait for the
/// next chunk is bounded by the absolute `deadline` and by `live()`; an error here makes ingestion
/// drop its partial file. Dropping the reader drops the body, which ends the upload.
struct BodyReader<S, L> {
    stream: S,
    handle: tokio::runtime::Handle,
    pending: axum::body::Bytes,
    deadline: tokio::time::Instant,
    slice: std::time::Duration,
    live: L,
}

impl<S, L> BodyReader<S, L> {
    fn new(stream: S, handle: tokio::runtime::Handle, deadline: tokio::time::Instant, slice: std::time::Duration, live: L) -> Self {
        Self { stream, handle, pending: axum::body::Bytes::new(), deadline, slice, live }
    }
}

impl<S, L> std::io::Read for BodyReader<S, L>
where
    S: futures_core::Stream<Item = Result<axum::body::Bytes, std::io::Error>> + Send + Unpin,
    L: Fn() -> bool,
{
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        while self.pending.is_empty() {
            if !(self.live)() {
                return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "plugin view retired"));
            }
            let now = tokio::time::Instant::now();
            if now >= self.deadline {
                return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "upload deadline"));
            }
            let wake = self.deadline.min(now + self.slice);
            match self.handle.block_on(tokio::time::timeout_at(wake, self.stream.next())) {
                Ok(Some(chunk)) => self.pending = chunk?,
                Ok(None) => return Ok(0),
                Err(_) => continue,
            }
        }
        let count = out.len().min(self.pending.len());
        out[..count].copy_from_slice(&self.pending.split_to(count));
        Ok(count)
    }
}

/// A display name safe for `Content-Disposition`: no separators, controls or quotes, never empty.
fn download_name(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(c, '/' | '\\' | '"' | ':' | '*' | '?' | '<' | '>' | '|')
        })
        .take(120)
        .collect();
    let trimmed = cleaned.trim().trim_start_matches('.');
    if trimmed.is_empty() {
        "media".to_string()
    } else {
        trimmed.to_string()
    }
}

/// `saveAs` for the web mirror: streams the plugin's own opened file as an attachment.
/// A refusal answers 409 with the typed `{ ok: false, code, reason }` body.
pub async fn host_action_download(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(request): Json<DownloadRequest>,
) -> Result<Response, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let svc = state.svc.clone();
    let stamp = Stamp::of(&headers);
    let opened = tokio::task::spawn_blocking(move || {
        view_host_download(
            &svc,
            &request.plugin_id,
            stamp.borrow(),
            request.handle.as_deref(),
        )
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let media = match opened {
        Ok(media) => media,
        Err(refusal) => return Ok((StatusCode::CONFLICT, Json(refusal)).into_response()),
    };
    let name = download_name(&media.file_name);
    let encoded: String = name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    // Exactly the registered length: bytes appended after registration are never sent, and a
    // file cut short after admission ends the stream with an error instead of a short download.
    let size = media.size;
    let file = tokio::fs::File::from_std(media.file);
    let body = futures_util::stream::unfold((file, size), |(mut file, remaining)| async move {
        use tokio::io::AsyncReadExt;
        if remaining == 0 {
            return None;
        }
        let mut chunk = vec![0u8; DOWNLOAD_CHUNK.min(remaining as usize)];
        match file.read(&mut chunk).await {
            Ok(0) => Some((
                Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "media changed")),
                (file, 0),
            )),
            Ok(n) => {
                chunk.truncate(n);
                Some((
                    Ok::<_, std::io::Error>(axum::body::Bytes::from(chunk)),
                    (file, remaining - n as u64),
                ))
            }
            Err(error) => Some((Err(error), (file, 0))),
        }
    })
    .boxed();
    let mut response = axum::body::Body::from_stream(body).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(size));
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Ok(disposition) =
        HeaderValue::from_str(&format!("attachment; filename*=UTF-8''{encoded}"))
    {
        headers.insert(header::CONTENT_DISPOSITION, disposition);
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::{download_name, owner_post, CallerIdentity};
    use axum::{body::Body, http::Request, http::StatusCode, Extension, Router};
    use codeterm_service_core::mesh::types::AccessLevel;
    use codeterm_service_core::service::tab_credentials::TabCaller;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use tower::ServiceExt;

    #[tokio::test]
    async fn host_action_routes_admit_only_the_local_owner_before_the_handler() {
        let callers = [
            (Some(CallerIdentity::Local), StatusCode::OK),
            (
                Some(CallerIdentity::Tab(TabCaller {
                    tab_id: "agent".into(),
                    binding_nonce: "nonce".into(),
                })),
                StatusCode::FORBIDDEN,
            ),
            (
                Some(CallerIdentity::Peer {
                    instance_id: "peer".into(),
                    access_level: AccessLevel::ReadOnly,
                    relay_protocol_version: 0,
                }),
                StatusCode::FORBIDDEN,
            ),
            (
                Some(CallerIdentity::ServicePush {
                    service_id: "svc".into(),
                }),
                StatusCode::FORBIDDEN,
            ),
            (None, StatusCode::FORBIDDEN),
        ];
        for (caller, expected) in callers {
            let reached = Arc::new(AtomicBool::new(false));
            let seen = reached.clone();
            let mut router: Router = Router::new().route(
                "/api/plugins/view-host-action",
                owner_post(move || async move {
                    seen.store(true, Ordering::SeqCst);
                    StatusCode::OK
                }),
            );
            let label = format!("{caller:?}");
            if let Some(caller) = caller {
                router = router.layer(Extension(caller));
            }
            let response = router
                .oneshot(
                    Request::post("/api/plugins/view-host-action")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected, "{label}");
            assert_eq!(
                reached.load(Ordering::SeqCst),
                expected == StatusCode::OK,
                "{label}"
            );
        }
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap()
    }

    type Chunk = Result<axum::body::Bytes, std::io::Error>;

    #[test]
    fn upload_reader_passes_chunks_through_and_ends_with_the_body() {
        use std::io::Read;
        let runtime = runtime();
        let stream = futures_util::stream::iter(vec![
            Ok::<_, std::io::Error>(axum::body::Bytes::from_static(b"png")),
            Ok(axum::body::Bytes::from_static(b"-bytes")),
        ]);
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(60);
        let mut reader = super::BodyReader::new(stream, runtime.handle().clone(), deadline, super::LIVENESS_SLICE, || true);
        let mut body = Vec::new();
        reader.read_to_end(&mut body).unwrap();
        assert_eq!(body, b"png-bytes");
    }

    #[test]
    fn upload_reader_never_waits_past_the_ingestion_deadline() {
        use std::io::Read;
        let runtime = runtime();
        // A body that never sends another chunk, with the absolute deadline already reached.
        let stream = futures_util::stream::pending::<Chunk>();
        let mut reader = super::BodyReader::new(stream, runtime.handle().clone(), tokio::time::Instant::now(), super::LIVENESS_SLICE, || true);
        let error = reader.read(&mut [0; 8]).expect_err("stalled upload");
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    }

    #[test]
    fn upload_reader_stops_waiting_once_the_document_is_retired() {
        use std::io::Read;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let runtime = runtime();
        let stream = futures_util::stream::pending::<Chunk>();
        let checks = std::sync::Arc::new(AtomicUsize::new(0));
        let seen = checks.clone();
        // Live for the first two checks, retired from the third; a zero slice makes each wait end at once.
        let live = move || seen.fetch_add(1, Ordering::SeqCst) < 2;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(60);
        let mut reader = super::BodyReader::new(stream, runtime.handle().clone(), deadline, std::time::Duration::ZERO, live);
        let error = reader.read(&mut [0; 8]).expect_err("retired document");
        assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
        assert_eq!(checks.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn web_input_names_decode_percent_encoded_utf8_within_bounds() {
        let named = |value: &str| {
            let mut headers = axum::http::HeaderMap::new();
            headers.insert(super::INPUT_NAME_HEADER, value.parse().unwrap());
            super::input_name(&headers)
        };
        assert_eq!(named("%D0%BB%D0%B8%D1%81%D0%B0.png").as_deref(), Some("лиса.png"));
        assert_eq!(named("fox.png").as_deref(), Some("fox.png"));
        assert_eq!(named("%ZZ"), None);
        assert_eq!(named("%D0"), None, "a cut multibyte character is refused");
        assert_eq!(named(&"a".repeat(121)), None, "more than 120 decoded characters");
        assert_eq!(named(&"%41".repeat(121)), None, "more than the encoded byte cap");
        assert_eq!(super::input_name(&axum::http::HeaderMap::new()), None);
    }

    #[test]
    fn dialog_paths_must_be_absolute_regular_files() {
        let dir = std::env::temp_dir().join(format!("domios-dialog-path-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("fox.png");
        std::fs::write(&file, b"png").unwrap();
        assert!(super::open_dialog_path(&file.to_string_lossy()).is_ok());
        assert!(super::open_dialog_path("fox.png").is_err(), "relative");
        assert!(super::open_dialog_path(&dir.to_string_lossy()).is_err(), "a folder");
        assert!(super::open_dialog_path(&dir.join("missing.png").to_string_lossy()).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn download_names_lose_separators_quotes_and_leading_dots() {
        assert_eq!(download_name("frame.png"), "frame.png");
        assert_eq!(download_name("../../etc/passwd"), "etcpasswd");
        assert_eq!(download_name("..\\a\"b\r\n.mp4"), "ab.mp4");
        assert_eq!(download_name("  "), "media");
        assert_eq!(download_name(".hidden"), "hidden");
    }
}
