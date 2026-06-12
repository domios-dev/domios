//! Voice transcription via local whisper daemon.
//!
//! Posts an audio file path to `{daemon_url}/transcribe` and parses the
//! returned text. A short-lived per-URL health cache prevents us from
//! pummeling a known-down daemon when many voice messages arrive at once.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Deserialize;

use crate::config::schema::VoiceTranscribeConfig;

#[derive(Debug, Clone)]
pub struct Transcript {
    pub text: String,
}

#[derive(Debug)]
pub enum TranscribeError {
    Disabled,
    EmptyPath,
    InvalidUrl(String),
    HealthFailed(String),
    RequestFailed(String),
    HttpStatus(u16),
    ParseError(String),
    Empty,
}

impl std::fmt::Display for TranscribeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disabled => write!(f, "voice transcription disabled"),
            Self::EmptyPath => write!(f, "empty attachment path"),
            Self::InvalidUrl(u) => write!(f, "invalid daemon URL: {}", u),
            Self::HealthFailed(e) => write!(f, "daemon health check failed: {}", e),
            Self::RequestFailed(e) => write!(f, "transcribe request failed: {}", e),
            Self::HttpStatus(c) => write!(f, "transcribe returned HTTP {}", c),
            Self::ParseError(e) => write!(f, "failed to parse response: {}", e),
            Self::Empty => write!(f, "empty transcript"),
        }
    }
}

impl std::error::Error for TranscribeError {}

#[derive(Deserialize)]
struct TranscribeResponse {
    text: Option<String>,
}

#[derive(Clone, Copy)]
struct HealthEntry {
    healthy: bool,
    checked_at: Instant,
}

fn health_cache() -> &'static Mutex<HashMap<String, HealthEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, HealthEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cached_health(url: &str, ttl: Duration) -> Option<bool> {
    let map = health_cache().lock();
    map.get(url)
        .filter(|e| e.checked_at.elapsed() < ttl)
        .map(|e| e.healthy)
}

fn store_health(url: &str, healthy: bool) {
    let mut map = health_cache().lock();
    map.insert(
        url.to_string(),
        HealthEntry {
            healthy,
            checked_at: Instant::now(),
        },
    );
}

/// Reset the health cache. Test-only escape hatch — production code
/// should rely on the TTL.
#[cfg(test)]
pub fn reset_health_cache() {
    health_cache().lock().clear();
}

fn build_client(timeout: Duration) -> Result<reqwest::Client, TranscribeError> {
    reqwest::Client::builder()
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| TranscribeError::RequestFailed(e.to_string()))
}

async fn check_health(client: &reqwest::Client, daemon_url: &str) -> Result<(), TranscribeError> {
    let url = format!("{}/healthz", daemon_url.trim_end_matches('/'));
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| TranscribeError::HealthFailed(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(TranscribeError::HealthFailed(format!(
            "HTTP {}",
            resp.status().as_u16()
        )));
    }
    Ok(())
}

pub async fn transcribe(
    path: &Path,
    cfg: &VoiceTranscribeConfig,
) -> Result<Transcript, TranscribeError> {
    if !cfg.enabled {
        return Err(TranscribeError::Disabled);
    }
    if path.as_os_str().is_empty() {
        return Err(TranscribeError::EmptyPath);
    }

    if !is_valid_http_url(&cfg.daemon_url) {
        return Err(TranscribeError::InvalidUrl(cfg.daemon_url.clone()));
    }

    let timeout = Duration::from_millis(cfg.timeout_ms.max(1));
    let client = build_client(timeout)?;
    let ttl = Duration::from_secs(cfg.health_check_ttl_secs.max(1));

    let cached = cached_health(&cfg.daemon_url, ttl);
    if cached == Some(false) {
        return Err(TranscribeError::HealthFailed("cached unhealthy".into()));
    }
    if cached.is_none() {
        match check_health(&client, &cfg.daemon_url).await {
            Ok(()) => store_health(&cfg.daemon_url, true),
            Err(e) => {
                store_health(&cfg.daemon_url, false);
                return Err(e);
            }
        }
    }

    let endpoint = format!("{}/transcribe", cfg.daemon_url.trim_end_matches('/'));
    let mut body = serde_json::json!({ "path": path_to_string(path) });
    if !cfg.language.trim().is_empty() {
        body["language"] = serde_json::Value::String(cfg.language.clone());
    }

    let resp = client
        .post(&endpoint)
        .json(&body)
        .send()
        .await
        .map_err(|e| TranscribeError::RequestFailed(e.to_string()))?;

    let status = resp.status();
    if !status.is_success() {
        return Err(TranscribeError::HttpStatus(status.as_u16()));
    }
    let parsed: TranscribeResponse = resp
        .json()
        .await
        .map_err(|e| TranscribeError::ParseError(e.to_string()))?;
    let text = parsed.text.unwrap_or_default().trim().to_string();
    if text.is_empty() {
        return Err(TranscribeError::Empty);
    }
    Ok(Transcript { text })
}

/// Build the agent-facing attachment line for a file, transcribing voice when
/// enabled. Shared by the Telegram bridge and the chat composer so both emit
/// the same `-=-codeterm:<kind> ...-=-` marker shape.
pub async fn format_attachment_line(
    path: &str,
    kind: &str,
    filename: Option<&str>,
    cfg: &VoiceTranscribeConfig,
) -> String {
    use crate::message_markers as mk;
    if kind == "voice" && !path.is_empty() {
        if !cfg.enabled {
            // Tell the agent (and the user reading the chat) that a transcriber
            // daemon is needed and where to find it — instead of a silent path.
            return mk::voice(
                path,
                None,
                Some("no transcriber daemon configured; run `codeterm docs TRANSCRIBE` for setup"),
            );
        }
        match transcribe(Path::new(path), cfg).await {
            Ok(t) => return mk::voice(path, Some(&t.text), None),
            Err(e) => {
                log::warn!("[voice] transcribe failed for {}: {}", path, e);
                return mk::voice(
                    path,
                    None,
                    Some(&format!(
                        "transcription unavailable: {} — run `codeterm docs TRANSCRIBE` to set up a daemon",
                        e
                    )),
                );
            }
        }
    }
    // Non-voice attachment (photo/document/video/…): keep the real kind.
    mk::Marker::new(kind)
        .attr("file", path)
        .attr_opt("name", filename)
        .build()
}

fn path_to_string(path: &Path) -> String {
    let buf = PathBuf::from(path);
    buf.to_string_lossy().to_string()
}

fn is_valid_http_url(url: &str) -> bool {
    let lower = url.trim();
    let rest = lower
        .strip_prefix("http://")
        .or_else(|| lower.strip_prefix("https://"));
    match rest {
        Some(host_and_more) => {
            let host = host_and_more.split('/').next().unwrap_or("").trim();
            !host.is_empty() && !host.contains(' ')
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        routing::{get, post},
        Json, Router,
    };
    use std::net::SocketAddr;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;

    struct FakeServer {
        addr: SocketAddr,
        shutdown: Option<oneshot::Sender<()>>,
        handle: tokio::task::JoinHandle<()>,
    }

    impl FakeServer {
        fn url(&self) -> String {
            format!("http://{}", self.addr)
        }
        async fn stop(mut self) {
            if let Some(tx) = self.shutdown.take() {
                let _ = tx.send(());
            }
            let _ = self.handle.await;
        }
    }

    async fn spawn_server(router: Router) -> FakeServer {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local_addr");
        let (tx, rx) = oneshot::channel::<()>();
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    let _ = rx.await;
                })
                .await;
        });
        FakeServer {
            addr,
            shutdown: Some(tx),
            handle,
        }
    }

    fn cfg_for(url: &str) -> VoiceTranscribeConfig {
        VoiceTranscribeConfig {
            enabled: true,
            daemon_url: url.to_string(),
            timeout_ms: 5_000,
            language: String::new(),
            fallback_raw_on_error: true,
            health_check_ttl_secs: 60,
        }
    }

    #[tokio::test]
    async fn ok_returns_text() {
        reset_health_cache();
        let router = Router::new()
            .route("/healthz", get(|| async { "ok" }))
            .route(
                "/transcribe",
                post(|| async { Json(serde_json::json!({ "text": "hello world" })) }),
            );
        let server = spawn_server(router).await;
        let cfg = cfg_for(&server.url());

        let result = transcribe(Path::new("/fake/audio.oga"), &cfg).await;
        assert!(
            matches!(&result, Ok(t) if t.text == "hello world"),
            "got {:?}",
            result
        );
        server.stop().await;
    }

    #[tokio::test]
    async fn http_5xx_is_error() {
        reset_health_cache();
        let router = Router::new()
            .route("/healthz", get(|| async { "ok" }))
            .route(
                "/transcribe",
                post(|| async { (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "boom") }),
            );
        let server = spawn_server(router).await;
        let cfg = cfg_for(&server.url());

        let result = transcribe(Path::new("/fake/audio.oga"), &cfg).await;
        assert!(matches!(result, Err(TranscribeError::HttpStatus(500))));
        server.stop().await;
    }

    #[tokio::test]
    async fn empty_text_is_error() {
        reset_health_cache();
        let router = Router::new()
            .route("/healthz", get(|| async { "ok" }))
            .route(
                "/transcribe",
                post(|| async { Json(serde_json::json!({ "text": "   " })) }),
            );
        let server = spawn_server(router).await;
        let cfg = cfg_for(&server.url());

        let result = transcribe(Path::new("/fake/audio.oga"), &cfg).await;
        assert!(
            matches!(result, Err(TranscribeError::Empty)),
            "got {:?}",
            result
        );
        server.stop().await;
    }

    #[tokio::test]
    async fn no_listener_health_fails() {
        reset_health_cache();
        // Bind to ephemeral port then drop listener so nothing answers.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let cfg = cfg_for(&format!("http://{}", addr));

        let result = transcribe(Path::new("/fake/audio.oga"), &cfg).await;
        assert!(
            matches!(result, Err(TranscribeError::HealthFailed(_))),
            "got {:?}",
            result
        );
    }

    #[tokio::test]
    async fn disabled_short_circuits() {
        reset_health_cache();
        let mut cfg = cfg_for("http://127.0.0.1:1");
        cfg.enabled = false;
        let result = transcribe(Path::new("/x.oga"), &cfg).await;
        assert!(matches!(result, Err(TranscribeError::Disabled)));
    }

    #[tokio::test]
    async fn empty_path_is_rejected() {
        reset_health_cache();
        let cfg = cfg_for("http://127.0.0.1:1");
        let result = transcribe(Path::new(""), &cfg).await;
        assert!(matches!(result, Err(TranscribeError::EmptyPath)));
    }

    #[tokio::test]
    async fn invalid_url_rejected() {
        reset_health_cache();
        let mut cfg = cfg_for("not a url");
        cfg.daemon_url = "not a url".into();
        let result = transcribe(Path::new("/x.oga"), &cfg).await;
        assert!(matches!(result, Err(TranscribeError::InvalidUrl(_))));
    }

    #[tokio::test]
    async fn health_cached_for_unhealthy() {
        reset_health_cache();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let url = format!("http://{}", addr);
        let cfg = cfg_for(&url);

        let _ = transcribe(Path::new("/x.oga"), &cfg).await;
        let cached = cached_health(&url, Duration::from_secs(60));
        assert_eq!(cached, Some(false));
    }

    #[tokio::test]
    async fn language_passed_when_set() {
        reset_health_cache();
        let saw_lang = Arc::new(AtomicBool::new(false));
        let saw_lang_clone = saw_lang.clone();
        let router = Router::new()
            .route("/healthz", get(|| async { "ok" }))
            .route(
                "/transcribe",
                post(move |Json(body): Json<serde_json::Value>| {
                    let saw = saw_lang_clone.clone();
                    async move {
                        if body.get("language").and_then(|v| v.as_str()) == Some("ru") {
                            saw.store(true, Ordering::SeqCst);
                        }
                        Json(serde_json::json!({ "text": "ok" }))
                    }
                }),
            );
        let server = spawn_server(router).await;
        let mut cfg = cfg_for(&server.url());
        cfg.language = "ru".into();

        let _ = transcribe(Path::new("/x.oga"), &cfg).await;
        assert!(saw_lang.load(Ordering::SeqCst));
        server.stop().await;
    }
}
