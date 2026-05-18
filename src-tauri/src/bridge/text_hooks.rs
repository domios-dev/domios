//! Generic pre-forward hook: external daemon intercepts inbound text
//! before it reaches General Agent. Daemon decides forward/replace/annotate.
//! Mirrors the voice_transcribe pattern (health-cache + timeout + fallback).

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::config::schema::TextHooksConfig;

#[derive(Debug, Clone, Serialize)]
pub struct PreForwardRequest {
    pub text: String,
    pub channel: String,
    pub user_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub msg_id: Option<String>,
    pub pane_id: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<HookAttachment>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HookAttachment {
    pub kind: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum HookDecision {
    Forward,
    Replace { respond: Vec<HookReply> },
    ForwardWithMetadata { metadata: serde_json::Value },
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct HookReply {
    pub channel: String,
    pub to: String,
    pub text: String,
}

#[derive(Debug)]
pub enum HookError {
    Disabled,
    InvalidUrl(String),
    HealthFailed(String),
    RequestFailed(String),
    HttpStatus(u16),
    ParseError(String),
}

impl std::fmt::Display for HookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disabled => write!(f, "text hooks disabled"),
            Self::InvalidUrl(u) => write!(f, "invalid daemon URL: {}", u),
            Self::HealthFailed(e) => write!(f, "daemon health check failed: {}", e),
            Self::RequestFailed(e) => write!(f, "pre_forward request failed: {}", e),
            Self::HttpStatus(c) => write!(f, "pre_forward returned HTTP {}", c),
            Self::ParseError(e) => write!(f, "failed to parse response: {}", e),
        }
    }
}

impl std::error::Error for HookError {}

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
    map.insert(url.to_string(), HealthEntry { healthy, checked_at: Instant::now() });
}

#[cfg(test)]
pub fn reset_health_cache() {
    health_cache().lock().clear();
}

fn build_client(timeout: Duration) -> Result<reqwest::Client, HookError> {
    reqwest::Client::builder()
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| HookError::RequestFailed(e.to_string()))
}

async fn check_health(client: &reqwest::Client, daemon_url: &str) -> Result<(), HookError> {
    let url = format!("{}/healthz", daemon_url.trim_end_matches('/'));
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| HookError::HealthFailed(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(HookError::HealthFailed(format!("HTTP {}", resp.status().as_u16())));
    }
    Ok(())
}

pub async fn pre_forward(
    req: &PreForwardRequest,
    cfg: &TextHooksConfig,
) -> Result<HookDecision, HookError> {
    if !cfg.enabled {
        return Err(HookError::Disabled);
    }
    if !is_valid_http_url(&cfg.daemon_url) {
        return Err(HookError::InvalidUrl(cfg.daemon_url.clone()));
    }

    let timeout = Duration::from_millis(cfg.timeout_ms.max(1));
    let client = build_client(timeout)?;
    let ttl = Duration::from_secs(cfg.health_check_ttl_secs.max(1));

    let cached = cached_health(&cfg.daemon_url, ttl);
    if cached == Some(false) {
        return Err(HookError::HealthFailed("cached unhealthy".into()));
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

    let endpoint = format!("{}/pre_forward", cfg.daemon_url.trim_end_matches('/'));
    let resp = client
        .post(&endpoint)
        .json(req)
        .send()
        .await
        .map_err(|e| HookError::RequestFailed(e.to_string()))?;

    let status = resp.status();
    if !status.is_success() {
        return Err(HookError::HttpStatus(status.as_u16()));
    }

    resp.json::<HookDecision>()
        .await
        .map_err(|e| HookError::ParseError(e.to_string()))
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
    use axum::{routing::{get, post}, Json, Router};
    use std::net::SocketAddr;
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;

    struct FakeServer {
        addr: SocketAddr,
        shutdown: Option<oneshot::Sender<()>>,
        handle: tokio::task::JoinHandle<()>,
    }

    impl FakeServer {
        fn url(&self) -> String { format!("http://{}", self.addr) }
        async fn stop(mut self) {
            if let Some(tx) = self.shutdown.take() { let _ = tx.send(()); }
            let _ = self.handle.await;
        }
    }

    async fn spawn_server(router: Router) -> FakeServer {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local_addr");
        let (tx, rx) = oneshot::channel::<()>();
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async move { let _ = rx.await; })
                .await;
        });
        FakeServer { addr, shutdown: Some(tx), handle }
    }

    fn cfg_for(url: &str) -> TextHooksConfig {
        TextHooksConfig {
            enabled: true,
            daemon_url: url.to_string(),
            timeout_ms: 5_000,
            health_check_ttl_secs: 60,
            fallback_on_error: "forward".to_string(),
        }
    }

    fn req() -> PreForwardRequest {
        PreForwardRequest {
            text: "hello".to_string(),
            channel: "telegram".to_string(),
            user_id: "1".to_string(),
            msg_id: Some("42".to_string()),
            pane_id: "general".to_string(),
            attachments: Vec::new(),
        }
    }

    #[tokio::test]
    async fn forward_decision_parsed() {
        reset_health_cache();
        let router = Router::new()
            .route("/healthz", get(|| async { "ok" }))
            .route("/pre_forward", post(|| async {
                Json(serde_json::json!({"action": "forward"}))
            }));
        let server = spawn_server(router).await;
        let cfg = cfg_for(&server.url());

        let result = pre_forward(&req(), &cfg).await;
        assert!(matches!(result, Ok(HookDecision::Forward)), "got {:?}", result);
        server.stop().await;
    }

    #[tokio::test]
    async fn replace_decision_parses_replies() {
        reset_health_cache();
        let router = Router::new()
            .route("/healthz", get(|| async { "ok" }))
            .route("/pre_forward", post(|| async {
                Json(serde_json::json!({
                    "action": "replace",
                    "respond": [
                        {"channel": "telegram", "to": "100000001", "text": "task done"}
                    ]
                }))
            }));
        let server = spawn_server(router).await;
        let cfg = cfg_for(&server.url());

        let result = pre_forward(&req(), &cfg).await;
        match result {
            Ok(HookDecision::Replace { respond }) => {
                assert_eq!(respond.len(), 1);
                assert_eq!(respond[0].channel, "telegram");
                assert_eq!(respond[0].to, "100000001");
                assert_eq!(respond[0].text, "task done");
            }
            other => panic!("expected Replace, got {:?}", other),
        }
        server.stop().await;
    }

    #[tokio::test]
    async fn metadata_decision_parses_arbitrary_json() {
        reset_health_cache();
        let router = Router::new()
            .route("/healthz", get(|| async { "ok" }))
            .route("/pre_forward", post(|| async {
                Json(serde_json::json!({
                    "action": "forward_with_metadata",
                    "metadata": {"router_label": "light_cmd", "extracted_action": "ha.mjs floor 1 on"}
                }))
            }));
        let server = spawn_server(router).await;
        let cfg = cfg_for(&server.url());

        let result = pre_forward(&req(), &cfg).await;
        match result {
            Ok(HookDecision::ForwardWithMetadata { metadata }) => {
                assert_eq!(metadata.get("router_label").and_then(|v| v.as_str()), Some("light_cmd"));
            }
            other => panic!("expected ForwardWithMetadata, got {:?}", other),
        }
        server.stop().await;
    }

    #[tokio::test]
    async fn http_5xx_is_error() {
        reset_health_cache();
        let router = Router::new()
            .route("/healthz", get(|| async { "ok" }))
            .route("/pre_forward", post(|| async {
                (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "boom")
            }));
        let server = spawn_server(router).await;
        let cfg = cfg_for(&server.url());

        let result = pre_forward(&req(), &cfg).await;
        assert!(matches!(result, Err(HookError::HttpStatus(500))));
        server.stop().await;
    }

    #[tokio::test]
    async fn no_listener_health_fails_and_caches() {
        reset_health_cache();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let cfg = cfg_for(&format!("http://{}", addr));

        let _ = pre_forward(&req(), &cfg).await;
        let cached = cached_health(&cfg.daemon_url, Duration::from_secs(60));
        assert_eq!(cached, Some(false));
    }

    #[tokio::test]
    async fn disabled_short_circuits() {
        reset_health_cache();
        let mut cfg = cfg_for("http://127.0.0.1:1");
        cfg.enabled = false;
        let result = pre_forward(&req(), &cfg).await;
        assert!(matches!(result, Err(HookError::Disabled)));
    }

    #[tokio::test]
    async fn invalid_url_rejected() {
        reset_health_cache();
        let mut cfg = cfg_for("not a url");
        cfg.daemon_url = "not a url".into();
        let result = pre_forward(&req(), &cfg).await;
        assert!(matches!(result, Err(HookError::InvalidUrl(_))));
    }

    #[tokio::test]
    async fn body_carries_request_fields() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        reset_health_cache();
        let saw = Arc::new(AtomicBool::new(false));
        let saw_clone = saw.clone();
        let router = Router::new()
            .route("/healthz", get(|| async { "ok" }))
            .route("/pre_forward", post(move |Json(body): Json<serde_json::Value>| {
                let saw = saw_clone.clone();
                async move {
                    let ok = body.get("text").and_then(|v| v.as_str()) == Some("hello")
                        && body.get("channel").and_then(|v| v.as_str()) == Some("telegram")
                        && body.get("user_id").and_then(|v| v.as_str()) == Some("1")
                        && body.get("msg_id").and_then(|v| v.as_str()) == Some("42");
                    if ok { saw.store(true, Ordering::SeqCst); }
                    Json(serde_json::json!({"action": "forward"}))
                }
            }));
        let server = spawn_server(router).await;
        let cfg = cfg_for(&server.url());

        let _ = pre_forward(&req(), &cfg).await;
        assert!(saw.load(Ordering::SeqCst));
        server.stop().await;
    }
}
