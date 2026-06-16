use super::*;
use crate::config::schema::VoiceTranscribeConfig;
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

fn engine_for(url: &str) -> HttpEngine {
    HttpEngine::from_config(&VoiceTranscribeConfig {
        enabled: true,
        daemon_url: url.to_string(),
        timeout_ms: 5_000,
        language: String::new(),
        fallback_raw_on_error: true,
        health_check_ttl_secs: 60,
        active: "builtin-http".into(),
    })
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
    let engine = engine_for(&server.url());

    let result = engine
        .transcribe(Path::new("/fake/audio.oga"), "")
        .await;
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
    let engine = engine_for(&server.url());

    let result = engine.transcribe(Path::new("/fake/audio.oga"), "").await;
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
    let engine = engine_for(&server.url());

    let result = engine.transcribe(Path::new("/fake/audio.oga"), "").await;
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
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let engine = engine_for(&format!("http://{}", addr));

    let result = engine.transcribe(Path::new("/fake/audio.oga"), "").await;
    assert!(
        matches!(result, Err(TranscribeError::HealthFailed(_))),
        "got {:?}",
        result
    );
}

#[tokio::test]
async fn disabled_short_circuits() {
    reset_health_cache();
    let mut engine = engine_for("http://127.0.0.1:1");
    engine.enabled = false;
    let result = engine.transcribe(Path::new("/x.oga"), "").await;
    assert!(matches!(result, Err(TranscribeError::Disabled)));
}

#[tokio::test]
async fn empty_path_is_rejected() {
    reset_health_cache();
    let engine = engine_for("http://127.0.0.1:1");
    let result = engine.transcribe(Path::new(""), "").await;
    assert!(matches!(result, Err(TranscribeError::EmptyPath)));
}

#[tokio::test]
async fn invalid_url_rejected() {
    reset_health_cache();
    let engine = engine_for("not a url");
    let result = engine.transcribe(Path::new("/x.oga"), "").await;
    assert!(matches!(result, Err(TranscribeError::InvalidUrl(_))));
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
    let engine = engine_for(&server.url());

    let _ = engine.transcribe(Path::new("/x.oga"), "ru").await;
    assert!(saw_lang.load(Ordering::SeqCst));
    server.stop().await;
}
