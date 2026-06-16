#![cfg(test)]
use super::*;

#[test]
fn test_create_outbound_channel() {
    let (sender, _rx) = create_outbound_channel();
    assert!(sender.lock().is_some());
}

use super::text_hooks;
use crate::config::schema::TextHooksConfig;
use axum::{
    routing::{get, post},
    Json, Router,
};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

fn text_msg() -> InboundMessage {
    InboundMessage {
        service: "telegram".into(),
        user_id: 100000001,
        user_name: "test_user".into(),
        text: "do the task".into(),
        attachments: vec![],
        message_id: Some(12345),
        reply_ctx: ReplyContext::Telegram { chat_id: 100000001 },
    }
}

async fn spawn_hook_server(
    router: Router,
) -> (String, oneshot::Sender<()>, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let (tx, rx) = oneshot::channel::<()>();
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            })
            .await;
    });
    (format!("http://{}", addr), tx, handle)
}

fn cfg(url: &str) -> TextHooksConfig {
    TextHooksConfig {
        enabled: true,
        daemon_url: url.into(),
        timeout_ms: 5_000,
        health_check_ttl_secs: 60,
        fallback_on_error: "forward".into(),
    }
}

#[tokio::test]
async fn apply_hook_forward_passes_through() {
    text_hooks::reset_health_cache();
    let router = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route(
            "/pre_forward",
            post(|| async { Json(serde_json::json!({"action": "forward"})) }),
        );
    let (url, shutdown, handle) = spawn_hook_server(router).await;
    let (outbound, _rx) = create_outbound_channel();

    let outcome = apply_hook(&outbound, &text_msg(), "general", "do the task", &cfg(&url)).await;
    assert!(matches!(outcome, HookOutcome::Forward));

    let _ = shutdown.send(());
    let _ = handle.await;
}

#[tokio::test]
async fn apply_hook_replace_emits_outbound() {
    text_hooks::reset_health_cache();
    let router = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route(
            "/pre_forward",
            post(|| async {
                Json(serde_json::json!({
                    "action": "replace",
                    "respond": [
                        {"channel": "telegram", "to": "100000001", "text": "task done"}
                    ]
                }))
            }),
        );
    let (url, shutdown, handle) = spawn_hook_server(router).await;
    let (outbound, mut rx) = create_outbound_channel();

    let outcome = apply_hook(&outbound, &text_msg(), "general", "do the task", &cfg(&url)).await;
    assert!(matches!(outcome, HookOutcome::Replaced));

    // Daemon's reply must land in the outbound channel as a Telegram message.
    let received = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .expect("outbound recv timed out")
        .expect("channel closed");
    assert_eq!(received.user_id, Some(100000001));
    match received.content {
        OutboundContent::Text { text } => assert!(text.contains("task done")),
        other => panic!("expected Text, got {:?}", other),
    }

    let _ = shutdown.send(());
    let _ = handle.await;
}

#[tokio::test]
async fn apply_hook_metadata_appends_annotation() {
    text_hooks::reset_health_cache();
    let router = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route(
            "/pre_forward",
            post(|| async {
                Json(serde_json::json!({
                    "action": "forward_with_metadata",
                    "metadata": {"router_label": "light_cmd"}
                }))
            }),
        );
    let (url, shutdown, handle) = spawn_hook_server(router).await;
    let (outbound, _rx) = create_outbound_channel();

    let body = "включи свет";
    let outcome = apply_hook(&outbound, &text_msg(), "general", body, &cfg(&url)).await;
    match outcome {
        HookOutcome::Annotated(annotated) => {
            assert!(annotated.starts_with(body));
            assert!(annotated.contains("-=-codeterm:hook-=-"));
            assert!(annotated.contains("light_cmd"));
        }
        other => panic!(
            "expected Annotated, got matches={}",
            matches!(other, HookOutcome::Forward)
        ),
    }

    let _ = shutdown.send(());
    let _ = handle.await;
}

#[tokio::test]
async fn apply_hook_disabled_skips_daemon() {
    text_hooks::reset_health_cache();
    let hits = std::sync::Arc::new(AtomicUsize::new(0));
    let hits_clone = hits.clone();
    let router = Router::new()
        .route(
            "/healthz",
            get(move || {
                let hits = hits_clone.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    "ok"
                }
            }),
        )
        .route(
            "/pre_forward",
            post(|| async { Json(serde_json::json!({"action": "forward"})) }),
        );
    let (url, shutdown, handle) = spawn_hook_server(router).await;
    let (outbound, _rx) = create_outbound_channel();

    let mut c = cfg(&url);
    c.enabled = false;
    let outcome = apply_hook(&outbound, &text_msg(), "general", "x", &c).await;
    assert!(matches!(outcome, HookOutcome::Forward));
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "disabled hook must not contact daemon"
    );

    let _ = shutdown.send(());
    let _ = handle.await;
}

#[tokio::test]
async fn apply_hook_fallback_drop_returns_replaced_on_error() {
    text_hooks::reset_health_cache();
    // No server bound to this port — every call fails health check.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let mut c = cfg(&format!("http://{}", addr));
    c.fallback_on_error = "drop".into();
    let (outbound, _rx) = create_outbound_channel();

    let outcome = apply_hook(&outbound, &text_msg(), "general", "x", &c).await;
    assert!(matches!(outcome, HookOutcome::Replaced));
}

#[test]
fn test_parse_inline_buttons() {
    let json = r#"[[{"text":"Да","data":"yes"},{"text":"Нет","data":"no"}]]"#;
    let buttons: super::InlineKeyboard = serde_json::from_str(json).unwrap();
    assert_eq!(buttons.len(), 1);
    assert_eq!(buttons[0].len(), 2);
    assert_eq!(buttons[0][0].text, "Да");
    assert_eq!(buttons[0][0].data.as_deref(), Some("yes"));
    assert!(buttons[0][0].url.is_none());
}

fn voice_msg(path: &str) -> InboundMessage {
    InboundMessage {
        service: "telegram".into(),
        user_id: 42,
        user_name: "tester".into(),
        text: String::new(),
        attachments: vec![InboundAttachment {
            path: path.into(),
            kind: "voice".into(),
            filename: None,
            mime_type: Some("audio/ogg".into()),
        }],
        message_id: Some(7),
        reply_ctx: ReplyContext::Telegram { chat_id: 99 },
    }
}

fn photo_msg(path: &str) -> InboundMessage {
    InboundMessage {
        service: "telegram".into(),
        user_id: 42,
        user_name: "tester".into(),
        text: String::new(),
        attachments: vec![InboundAttachment {
            path: path.into(),
            kind: "photo".into(),
            filename: Some("pic.jpg".into()),
            mime_type: Some("image/jpeg".into()),
        }],
        message_id: None,
        reply_ctx: ReplyContext::Telegram { chat_id: 99 },
    }
}

fn enabled_cfg(url: &str) -> VoiceTranscribeConfig {
    VoiceTranscribeConfig {
        enabled: true,
        daemon_url: url.into(),
        timeout_ms: 5_000,
        language: String::new(),
        fallback_raw_on_error: true,
        health_check_ttl_secs: 60,
        active: "builtin-http".into(),
    }
}

/// Build a transcribe registry holding the built-in HTTP engine from `cfg`.
fn registry_for(cfg: &VoiceTranscribeConfig) -> crate::transcribe::TranscribeRegistry {
    let reg = crate::transcribe::TranscribeRegistry::new();
    reg.register(
        crate::transcribe::http_engine::BUILTIN_HTTP_ID,
        std::sync::Arc::new(crate::transcribe::http_engine::HttpEngine::from_config(cfg)),
    );
    reg
}

async fn spawn_transcribe_server(
    text: &'static str,
) -> (
    std::net::SocketAddr,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    use axum::{
        routing::{get, post},
        Json, Router,
    };
    let router = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route(
            "/transcribe",
            post(move || async move { Json(serde_json::json!({ "text": text })) }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            })
            .await;
    });
    (addr, tx, handle)
}

#[tokio::test]
async fn body_includes_transcript_and_file_uri() {
    voice_transcribe::reset_health_cache();
    let (addr, stop, handle) = spawn_transcribe_server("привет мир").await;
    let cfg = enabled_cfg(&format!("http://{}", addr));

    let body = build_message_body(&voice_msg("/tmp/voice.oga"), &registry_for(&cfg), "builtin-http").await;
    // -=-codeterm:voice file=/tmp/voice.oga-=-привет мир — transcript is the payload.
    assert_eq!(body, "-=-codeterm:voice file=/tmp/voice.oga-=-привет мир");

    let _ = stop.send(());
    let _ = handle.await;
}

#[tokio::test]
async fn body_falls_back_to_raw_when_daemon_down() {
    voice_transcribe::reset_health_cache();
    // Reserve a port and drop the listener so nothing answers.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let cfg = enabled_cfg(&format!("http://{}", addr));

    let body = build_message_body(&voice_msg("/tmp/voice.oga"), &registry_for(&cfg), "builtin-http").await;
    // Daemon down → untranscribed voice marker; the readable note is the payload.
    assert!(
        body.starts_with("-=-codeterm:voice file=/tmp/voice.oga status=untranscribed-=-"),
        "body: {}",
        body
    );
    assert!(body.contains("transcription unavailable"), "body: {}", body);
}

#[tokio::test]
async fn disabled_cfg_notes_missing_transcriber() {
    voice_transcribe::reset_health_cache();
    let mut cfg = enabled_cfg("http://127.0.0.1:1");
    cfg.enabled = false;

    let body = build_message_body(&voice_msg("/tmp/voice.oga"), &registry_for(&cfg), "builtin-http").await;
    // Transcription off → untranscribed marker + readable note (with doc pointer).
    assert!(
        body.starts_with("-=-codeterm:voice file=/tmp/voice.oga status=untranscribed-=-"),
        "body: {}",
        body
    );
    // Engine present but disabled → transcribe errors; note carries the reason.
    assert!(
        body.contains("transcription unavailable"),
        "body: {}",
        body
    );
}

#[tokio::test]
async fn non_voice_attachments_not_transcribed() {
    voice_transcribe::reset_health_cache();
    // Daemon URL points nowhere; if we accidentally hit it, the test fails because we'd see fallback path. With photo kind we never call it.
    let cfg = enabled_cfg("http://127.0.0.1:1");

    let body = build_message_body(&photo_msg("/tmp/pic.jpg"), &registry_for(&cfg), "builtin-http").await;
    assert_eq!(body, "-=-codeterm:photo file=/tmp/pic.jpg name=pic.jpg-=-");
}

#[tokio::test]
async fn empty_attachment_path_skips_transcription() {
    voice_transcribe::reset_health_cache();
    let cfg = enabled_cfg("http://127.0.0.1:1");

    let body = build_message_body(&voice_msg(""), &registry_for(&cfg), "builtin-http").await;
    assert_eq!(body, "-=-codeterm:voice file=-=-");
}

#[tokio::test]
async fn multiple_voice_attachments_each_transcribed() {
    voice_transcribe::reset_health_cache();
    let (addr, stop, handle) = spawn_transcribe_server("ok").await;
    let cfg = enabled_cfg(&format!("http://{}", addr));

    let mut msg = voice_msg("/tmp/a.oga");
    msg.attachments.push(InboundAttachment {
        path: "/tmp/b.oga".into(),
        kind: "voice".into(),
        filename: None,
        mime_type: Some("audio/ogg".into()),
    });

    let body = build_message_body(&msg, &registry_for(&cfg), "builtin-http").await;
    // Two voice markers, each with "ok" as its transcript payload.
    assert_eq!(
        body.matches("-=-codeterm:voice").count(),
        2,
        "body: {}",
        body
    );
    assert_eq!(body.matches("-=-ok").count(), 2, "body: {}", body);
    assert!(body.contains("file=/tmp/a.oga"));
    assert!(body.contains("file=/tmp/b.oga"));

    let _ = stop.send(());
    let _ = handle.await;
}

#[tokio::test]
async fn malformed_url_notes_unavailable() {
    voice_transcribe::reset_health_cache();
    let mut cfg = enabled_cfg("not-a-url");
    cfg.daemon_url = "not a url".into();

    let body = build_message_body(&voice_msg("/tmp/voice.oga"), &registry_for(&cfg), "builtin-http").await;
    assert!(body.contains("transcription unavailable"), "body: {}", body);
    assert!(body.contains("file=/tmp/voice.oga"), "body: {}", body);
}

#[test]
fn test_reply_markup_serialization() {
    use crate::bridge::traits::{InlineButton, InlineKeyboard};
    let kb: InlineKeyboard = vec![vec![
        InlineButton {
            text: "Yes".into(),
            data: Some("yes".into()),
            url: None,
        },
        InlineButton {
            text: "No".into(),
            data: Some("no".into()),
            url: None,
        },
    ]];
    let payload = serde_json::json!({ "inline_keyboard": kb });
    let s = payload.to_string();
    assert!(
        s.contains("\"callback_data\""),
        "should serialize as callback_data: {}",
        s
    );
    assert!(
        !s.contains("\"data\""),
        "should not contain raw 'data' key: {}",
        s
    );
    let rows = payload["inline_keyboard"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].as_array().unwrap().len(), 2);
}

use parking_lot::Mutex as PLMutex;
use std::sync::Arc;

#[test]
fn telegram_marker_carries_reply_route() {
    let marked = crate::message_markers::telegram("100000001", Some("12345"), "hello");
    let parsed = crate::message_markers::parse(&marked).expect("telegram marker parses");

    assert_eq!(parsed.kind, "telegram");
    assert_eq!(parsed.attr("user"), Some("100000001"));
    assert_eq!(parsed.attr("msg"), Some("12345"));
    assert_eq!(parsed.payload, "hello");
}

#[test]
fn unsupported_bridge_service_is_rejected() {
    assert_eq!(ensure_supported_service("telegram"), Ok(()));
    assert_eq!(
        ensure_supported_service("discord"),
        Err("service unsupported")
    );
}

async fn spawn_fake_telegram(captured: Arc<PLMutex<Vec<serde_json::Value>>>) -> String {
    use axum::{routing::post, Json, Router};

    let captured_a = captured.clone();
    let captured_b = captured.clone();

    let app = Router::new()
        .route(
            "/bot:method",
            post(move |Json(body): Json<serde_json::Value>| {
                let captured = captured_a.clone();
                async move {
                    captured.lock().push(body);
                    Json(serde_json::json!({"ok": true, "result": {"message_id": 42}}))
                }
            }),
        )
        .fallback(move |Json(body): Json<serde_json::Value>| {
            let captured = captured_b.clone();
            async move {
                captured.lock().push(body);
                Json(serde_json::json!({"ok": true, "result": {"message_id": 42}}))
            }
        });

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{}", addr)
}

async fn spawn_fake_telegram_with_methods(
    captured: Arc<PLMutex<Vec<(String, serde_json::Value)>>>,
) -> String {
    use axum::{body, http::Request, Json, Router};

    let app = Router::new().fallback(move |req: Request<body::Body>| {
        let captured = captured.clone();
        async move {
            let path = req.uri().path().to_string();
            let bytes = body::to_bytes(req.into_body(), 1024 * 1024).await.unwrap();
            let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            captured.lock().push((path, body));
            Json(serde_json::json!({"ok": true, "result": {"message_id": 42}}))
        }
    });

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{}", addr)
}

#[tokio::test]
async fn outbound_text_routes_to_chat_and_append_edits_previous_message() {
    let captured = Arc::new(PLMutex::new(Vec::new()));
    let base = spawn_fake_telegram_with_methods(captured.clone()).await;
    let client = reqwest::Client::new();
    let mut chat_states = HashMap::new();

    route_text_outbound(
        &client,
        &base,
        "FAKETOKEN",
        100000001,
        "first",
        false,
        None,
        &mut chat_states,
    )
    .await;
    route_text_outbound(
        &client,
        &base,
        "FAKETOKEN",
        100000001,
        "second",
        true,
        None,
        &mut chat_states,
    )
    .await;

    let recorded = captured.lock();
    assert_eq!(recorded.len(), 2, "requests: {:?}", *recorded);
    assert!(recorded[0].0.ends_with("/sendMessage"));
    assert_eq!(
        recorded[0].1.get("chat_id").and_then(|v| v.as_i64()),
        Some(100000001)
    );
    assert_eq!(
        recorded[0].1.get("text").and_then(|v| v.as_str()),
        Some("first")
    );

    assert!(recorded[1].0.ends_with("/editMessageText"));
    assert_eq!(
        recorded[1].1.get("chat_id").and_then(|v| v.as_i64()),
        Some(100000001)
    );
    assert_eq!(
        recorded[1].1.get("message_id").and_then(|v| v.as_i64()),
        Some(42)
    );
    assert_eq!(
        recorded[1].1.get("text").and_then(|v| v.as_str()),
        Some("first\n\nsecond")
    );
}

#[tokio::test]
async fn integration_send_message_sanitizes_reproducer() {
    let captured = Arc::new(PLMutex::new(Vec::new()));
    let base = spawn_fake_telegram(captured.clone()).await;
    let client = reqwest::Client::new();

    let original = "<code>--valid-to <date></code>";
    let sanitized = super::super::sanitize::sanitize_html_for_telegram(original);
    let result = send_message(&client, &base, "FAKETOKEN", 12345, &sanitized).await;

    assert_eq!(result, Some(42), "send should succeed via fake server");
    let recorded = captured.lock();
    assert_eq!(
        recorded.len(),
        1,
        "must NOT trigger fallback retry, got {} requests",
        recorded.len()
    );
    let body = &recorded[0];
    assert_eq!(
        body.get("parse_mode").and_then(|v| v.as_str()),
        Some("HTML"),
        "first request must be HTML mode (cold-path fallback only)"
    );
    let text = body.get("text").and_then(|v| v.as_str()).unwrap();
    assert!(
        !text.contains("<date>"),
        "raw <date> must be sanitized; got: {}",
        text
    );
    assert!(
        text.contains("&lt;date&gt;"),
        "must contain escaped &lt;date&gt;; got: {}",
        text
    );
    assert!(
        text.contains("<code>"),
        "must keep <code> wrapper; got: {}",
        text
    );
}

#[tokio::test]
async fn integration_send_message_no_fallback_for_valid_html() {
    let captured = Arc::new(PLMutex::new(Vec::new()));
    let base = spawn_fake_telegram(captured.clone()).await;
    let client = reqwest::Client::new();

    let original = "<b>bold</b> and <i>italic</i>";
    let sanitized = super::super::sanitize::sanitize_html_for_telegram(original);
    assert_eq!(sanitized, original);

    let result = send_message(&client, &base, "FAKETOKEN", 12345, &sanitized).await;
    assert_eq!(result, Some(42));
    assert_eq!(captured.lock().len(), 1);
}
