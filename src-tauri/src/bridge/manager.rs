//! BridgeManager v4 — CLI-based bridge with typing indicator, auto-edit, markdown.
//!
//! Outbound: agents call `codeterm bridge send` → channel → Telegram API.
//! Inbound: Telegram → PTY with [Telegram: user_id] prefix.
//! Features: typing indicator on inbound, auto-edit recent messages, markdown formatting.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use parking_lot::Mutex;

use super::traits::{InboundAttachment, InboundMessage, InlineKeyboard, MessagingService, OutboundContent, ReplyContext};
use super::voice_transcribe;
use crate::config::schema::VoiceTranscribeConfig;
use crate::service::general_agent;
use crate::service::ServiceContext;

/// Auto-edit window — if a new send arrives within this time, edit the previous message.
const AUTO_EDIT_WINDOW_SECS: u64 = 60;

#[derive(Debug, Clone)]
pub struct OutboundMessage {
    pub user_id: Option<i64>,
    pub content: OutboundContent,
    /// If true, append text to the previous message instead of replacing.
    pub append: bool,
    /// Optional inline keyboard buttons.
    pub buttons: Option<InlineKeyboard>,
}

pub type OutboundSender = Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<OutboundMessage>>>>;

pub fn create_outbound_channel() -> (OutboundSender, tokio::sync::mpsc::UnboundedReceiver<OutboundMessage>) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (Arc::new(Mutex::new(Some(tx))), rx)
}

/// Track last sent message per chat for auto-edit and append.
struct ChatState {
    message_id: i64,
    sent_at: Instant,
    /// Accumulated text of the current message (for append mode).
    text: String,
}

pub struct BridgeManager {
    svc: ServiceContext,
}

impl BridgeManager {
    pub fn new(svc: ServiceContext) -> Self {
        Self { svc }
    }

    pub async fn start(
        self,
        services: Vec<Box<dyn MessagingService>>,
        mut outbound_rx: tokio::sync::mpsc::UnboundedReceiver<OutboundMessage>,
        cancel: Arc<tokio::sync::Notify>,
        status: Arc<dyn super::supervisor::BridgeStatusSink>,
    ) {
        let (tx, mut rx) = mpsc::channel::<InboundMessage>(64);
        let fallback_chat_id: Arc<Mutex<Option<i64>>> = Arc::new(Mutex::new(None));

        for service in services {
            let tx_clone = tx.clone();
            let id = service.id().to_string();
            let svc_cancel = cancel.clone();
            let svc_status = status.clone();
            tokio::spawn(async move {
                log::info!("[bridge] starting service: {}", id);
                service.start(tx_clone.clone(), svc_cancel, svc_status).await;
                log::info!("[bridge] service {} exited", id);
            });
        }

        let token = super::telegram::read_token().unwrap_or_default();
        let api_base = super::telegram::telegram_api_base();
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(15))
            .tcp_keepalive(std::time::Duration::from_secs(15))
            .build().unwrap_or_else(|_| reqwest::Client::new());

        // Keep tx alive — if we drop it and telegram service dies, rx closes and select! breaks.
        let _keep_tx = tx;

        // Per-chat state for auto-edit
        let mut chat_states: HashMap<i64, ChatState> = HashMap::new();

        log::info!("[bridge] manager v4 started (CLI + auto-edit + typing)");

        let fallback_clone = fallback_chat_id.clone();
        let svc = self.svc.clone();

        loop {
            tokio::select! {
                _ = cancel.notified() => {
                    log::info!("[bridge] manager received cancel; exiting outer loop");
                    return;
                }
                Some(msg) = rx.recv() => {
                    // Debounce: collect rapid-fire messages (user splitting text, Telegram auto-split)
                    let mut messages = vec![msg];
                    let debounce_until = tokio::time::Instant::now() + std::time::Duration::from_millis(300);
                    loop {
                        let remaining = debounce_until.saturating_duration_since(tokio::time::Instant::now());
                        if remaining.is_zero() { break; }
                        match tokio::time::timeout(remaining, rx.recv()).await {
                            Ok(Some(next)) => messages.push(next),
                            _ => break,
                        }
                    }

                    // Merge messages from same user
                    let merged_text: String = messages.iter()
                        .map(|m| m.text.as_str())
                        .filter(|t| !t.is_empty())
                        .collect::<Vec<_>>()
                        .join("\n");
                    let merged_attachments: Vec<_> = messages.iter()
                        .flat_map(|m| m.attachments.iter().cloned())
                        .collect();
                    let first = &messages[0];

                    log::info!("[bridge] inbound from {} ({}, {} msg, {} attachments): {}",
                        first.user_name, first.service, messages.len(), merged_attachments.len(),
                        &merged_text.chars().take(100).collect::<String>());

                    let ReplyContext::Telegram { chat_id } = &first.reply_ctx;
                    *fallback_clone.lock() = Some(*chat_id);
                    send_typing(&client, &api_base, &token, *chat_id).await;

                    let merged_msg = InboundMessage {
                        service: first.service.clone(),
                        user_id: first.user_id,
                        user_name: first.user_name.clone(),
                        text: merged_text,
                        attachments: merged_attachments,
                        message_id: first.message_id,
                        reply_ctx: first.reply_ctx.clone(),
                    };

                    let voice_cfg = crate::config::load_config().integrations.voice_transcribe.clone();
                    if let Err(e) = inject_message(&svc, &merged_msg, &voice_cfg).await {
                        log::error!("[bridge] inject failed: {}", e);
                    }
                }

                Some(out_msg) = outbound_rx.recv() => {
                    let chat_id = out_msg.user_id
                        .or_else(|| *fallback_clone.lock())
                        .unwrap_or(0);

                    if chat_id == 0 {
                        log::warn!("[bridge-outbound] no chat_id, dropping");
                        continue;
                    }

                    match out_msg.content {
                        OutboundContent::Text { ref text } => {
                            log::info!("[bridge-outbound] {} {} chars to {}",
                                if out_msg.append { "appending" } else { "sending" }, text.len(), chat_id);

                            // Build final text — append to previous or replace
                            let final_text = if out_msg.append {
                                if let Some(state) = chat_states.get(&chat_id) {
                                    if state.sent_at.elapsed().as_secs() < AUTO_EDIT_WINDOW_SECS {
                                        format!("{}\n\n{}", state.text, text)
                                    } else {
                                        text.clone()
                                    }
                                } else {
                                    text.clone()
                                }
                            } else {
                                text.clone()
                            };

                            // Edit only when --append flag is set AND there's a recent message to edit
                            let can_edit = out_msg.append && chat_states.get(&chat_id)
                                .map(|s| s.sent_at.elapsed().as_secs() < AUTO_EDIT_WINDOW_SECS)
                                .unwrap_or(false);

                            let sanitized = super::sanitize::sanitize_html_for_telegram(&final_text);

                            if can_edit && sanitized.len() <= super::telegram::TELEGRAM_MAX_LEN {
                                let msg_id = chat_states[&chat_id].message_id;
                                let edited = edit_message(&client, &api_base, &token, chat_id, msg_id, &sanitized).await;
                                if !edited {
                                    if let Some(new_id) = send_message(&client, &api_base, &token, chat_id, &sanitized).await {
                                        chat_states.insert(chat_id, ChatState { message_id: new_id, sent_at: Instant::now(), text: final_text });
                                    }
                                } else {
                                    if let Some(state) = chat_states.get_mut(&chat_id) {
                                        state.sent_at = Instant::now();
                                        state.text = final_text;
                                    }
                                }
                            } else {
                                let chunks: Vec<String> = super::response::chunk_message(&sanitized, super::telegram::TELEGRAM_MAX_LEN);
                                let mut last_id = None;
                                let last_idx = chunks.len().saturating_sub(1);
                                for (i, chunk) in chunks.iter().enumerate() {
                                    let btns = if i == last_idx { out_msg.buttons.as_ref() } else { None };
                                    last_id = send_message_with_buttons(&client, &api_base, &token, chat_id, chunk, btns).await;
                                }
                                if let Some(id) = last_id {
                                    chat_states.insert(chat_id, ChatState { message_id: id, sent_at: Instant::now(), text: final_text });
                                }
                            }
                        }
                        ref media_content => {
                            log::info!("[bridge-outbound] sending media {:?} to {}", std::mem::discriminant(media_content), chat_id);
                            if let Err(e) = send_media(&client, &api_base, &token, chat_id, media_content).await {
                                log::error!("[bridge-outbound] media send failed: {}", e);
                            }
                        }
                    }
                }

                else => {
                    log::error!("[bridge] select! else branch — both rx channels returned None. This should not happen.");
                    log::error!("[bridge] rx closed={}, outbound_rx will be checked next iteration", rx.is_closed());
                    // Don't break — sleep and retry. Channels may recover.
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }
    }
}

fn tg_url(base: &str, token: &str, method: &str) -> String {
    format!("{}/bot{}/{}", base.trim_end_matches('/'), token, method)
}

async fn send_typing(client: &reqwest::Client, base: &str, token: &str, chat_id: i64) {
    let _ = client
        .post(tg_url(base, token, "sendChatAction"))
        .json(&serde_json::json!({ "chat_id": chat_id, "action": "typing" }))
        .timeout(std::time::Duration::from_secs(5))
        .send().await;
}

async fn send_message(client: &reqwest::Client, base: &str, token: &str, chat_id: i64, text: &str) -> Option<i64> {
    send_message_with_buttons(client, base, token, chat_id, text, None).await
}

async fn send_message_with_buttons(client: &reqwest::Client, base: &str, token: &str, chat_id: i64, text: &str, buttons: Option<&InlineKeyboard>) -> Option<i64> {
    let mut payload = serde_json::json!({
        "chat_id": chat_id,
        "text": text,
        "parse_mode": "HTML",
        "disable_web_page_preview": true,
    });
    if let Some(kb) = buttons {
        payload["reply_markup"] = serde_json::json!({ "inline_keyboard": kb });
    }

    let resp = client
        .post(tg_url(base, token, "sendMessage"))
        .json(&payload)
        .timeout(std::time::Duration::from_secs(10))
        .send().await.ok()?;

    let body: serde_json::Value = resp.json().await.ok()?;

    if body.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let mut payload2 = serde_json::json!({
            "chat_id": chat_id,
            "text": text,
            "disable_web_page_preview": true,
        });
        if let Some(kb) = buttons {
            payload2["reply_markup"] = serde_json::json!({ "inline_keyboard": kb });
        }
        let resp2 = client
            .post(tg_url(base, token, "sendMessage"))
            .json(&payload2)
            .timeout(std::time::Duration::from_secs(10))
            .send().await.ok()?;
        let body2: serde_json::Value = resp2.json().await.ok()?;
        return body2.get("result").and_then(|r| r.get("message_id")).and_then(|v| v.as_i64());
    }

    body.get("result").and_then(|r| r.get("message_id")).and_then(|v| v.as_i64())
}

async fn edit_message(client: &reqwest::Client, base: &str, token: &str, chat_id: i64, message_id: i64, text: &str) -> bool {
    let resp = client
        .post(tg_url(base, token, "editMessageText"))
        .json(&serde_json::json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "text": text,
            "parse_mode": "HTML",
            "disable_web_page_preview": true,
        }))
        .timeout(std::time::Duration::from_secs(10))
        .send().await;

    match resp {
        Ok(r) => {
            let body: serde_json::Value = r.json().await.unwrap_or_default();
            if body.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                return true;
            }
            let desc = body.get("description").and_then(|v| v.as_str()).unwrap_or("");
            if desc.contains("message is not modified") {
                return true;
            }
            if desc.contains("parse") {
                let resp2 = client
                    .post(tg_url(base, token, "editMessageText"))
                    .json(&serde_json::json!({
                        "chat_id": chat_id,
                        "message_id": message_id,
                        "text": text,
                        "disable_web_page_preview": true,
                    }))
                    .timeout(std::time::Duration::from_secs(10))
                    .send().await;
                return resp2.map(|r| r.status().is_success()).unwrap_or(false);
            }
            log::warn!("[bridge] editMessage failed: {}", desc);
            false
        }
        Err(e) => {
            log::warn!("[bridge] editMessage error: {}", e);
            false
        }
    }
}

async fn send_media(client: &reqwest::Client, base: &str, token: &str, chat_id: i64, content: &OutboundContent) -> Result<(), String> {
    let (method, file_path, raw_caption, filename) = match content {
        OutboundContent::Photo { file, caption } => ("sendPhoto", file.as_str(), caption.as_deref(), None),
        OutboundContent::Document { file, caption, filename } => ("sendDocument", file.as_str(), caption.as_deref(), filename.as_deref()),
        OutboundContent::Voice { file, caption } => ("sendVoice", file.as_str(), caption.as_deref(), None),
        OutboundContent::Animation { file, caption } => ("sendAnimation", file.as_str(), caption.as_deref(), None),
        OutboundContent::Video { file, caption } => ("sendVideo", file.as_str(), caption.as_deref(), None),
        OutboundContent::Sticker { file } => ("sendSticker", file.as_str(), None, None),
        OutboundContent::Text { .. } => return Err("send_media called with Text content".into()),
    };
    let sanitized_caption = raw_caption.map(super::sanitize::sanitize_html_for_telegram);
    let caption = sanitized_caption.as_deref();

    // Read file
    let file_bytes = tokio::fs::read(file_path).await.map_err(|e| format!("Failed to read file {}: {}", file_path, e))?;
    let file_name = filename
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            std::path::Path::new(file_path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "file".to_string())
        });

    // Determine the multipart field name for the file
    let file_field = match method {
        "sendPhoto" => "photo",
        "sendDocument" => "document",
        "sendVoice" => "voice",
        "sendAnimation" => "animation",
        "sendVideo" => "video",
        "sendSticker" => "sticker",
        _ => "document",
    };

    let part = reqwest::multipart::Part::bytes(file_bytes).file_name(file_name);
    let mut form = reqwest::multipart::Form::new()
        .text("chat_id", chat_id.to_string())
        .part(file_field, part);

    if let Some(cap) = caption {
        form = form.text("caption", cap.to_string());
        form = form.text("parse_mode", "HTML".to_string());
    }

    let url = tg_url(base, token, method);
    let resp = client
        .post(&url)
        .multipart(form)
        .timeout(std::time::Duration::from_secs(30))
        .send().await
        .map_err(|e| format!("{} request failed: {}", method, e))?;

    let body: serde_json::Value = resp.json().await.map_err(|e| format!("{} parse failed: {}", method, e))?;
    if body.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let desc = body.get("description").and_then(|v| v.as_str()).unwrap_or("unknown error");
        // Retry without parse_mode if caption parsing failed
        if caption.is_some() && desc.contains("parse") {
            let file_bytes2 = tokio::fs::read(file_path).await.map_err(|e| format!("Re-read failed: {}", e))?;
            let file_name2 = std::path::Path::new(file_path)
                .file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "file".to_string());
            let part2 = reqwest::multipart::Part::bytes(file_bytes2).file_name(file_name2);
            let mut form2 = reqwest::multipart::Form::new()
                .text("chat_id", chat_id.to_string())
                .part(file_field, part2);
            if let Some(cap) = caption {
                form2 = form2.text("caption", cap.to_string());
            }
            let resp2 = client.post(&url).multipart(form2)
                .timeout(std::time::Duration::from_secs(30))
                .send().await.map_err(|e| e.to_string())?;
            let body2: serde_json::Value = resp2.json().await.map_err(|e| e.to_string())?;
            if body2.get("ok").and_then(|v| v.as_bool()) != Some(true) {
                return Err(format!("{} failed: {}", method, body2.get("description").and_then(|v| v.as_str()).unwrap_or("unknown")));
            }
        } else {
            return Err(format!("{} failed: {}", method, desc));
        }
    }
    Ok(())
}

/// Build the paste body for an inbound message, transcribing voice
/// attachments inline when enabled. Pure async — no PTY/lock access.
pub async fn build_message_body(msg: &InboundMessage, voice_cfg: &VoiceTranscribeConfig) -> String {
    let mut parts = Vec::new();
    if !msg.text.is_empty() {
        parts.push(msg.text.clone());
    }
    for att in &msg.attachments {
        parts.push(format_attachment_line(att, voice_cfg).await);
    }
    parts.join("\n")
}

async fn format_attachment_line(att: &InboundAttachment, voice_cfg: &VoiceTranscribeConfig) -> String {
    let desc = if let Some(ref name) = att.filename {
        format!("[{}: {}]", att.kind, name)
    } else {
        format!("[{}]", att.kind)
    };
    let file_uri = format!("file://{}", att.path.replace('\\', "/"));

    if att.kind == "voice" && voice_cfg.enabled && !att.path.is_empty() {
        match voice_transcribe::transcribe(std::path::Path::new(&att.path), voice_cfg).await {
            Ok(t) => {
                let escaped = t.text.replace('"', "\\\"");
                return format!("{} (voice transcript: \"{}\") {}", desc, escaped, file_uri);
            }
            Err(e) => {
                log::warn!("[bridge] voice transcribe failed for {}: {}", att.path, e);
                if !voice_cfg.fallback_raw_on_error {
                    return format!("{} (transcription unavailable: {})", desc, e);
                }
            }
        }
    }
    format!("{} {}", desc, file_uri)
}

async fn inject_message(
    svc: &ServiceContext,
    msg: &InboundMessage,
    voice_cfg: &VoiceTranscribeConfig,
) -> Result<(), String> {
    let (pane_id, agent_type) = {
        let mux = svc.mux.lock();
        let pid = mux.resolve_pane_id(general_agent::GENERAL_ALIAS)
            .ok_or("General Agent not found")?;
        let at = mux.get_pane(&pid).and_then(|p| p.agent_type.clone());
        (pid, at)
    };

    let body = build_message_body(msg, voice_cfg).await;
    let msg_id_suffix = msg.message_id.map(|id| format!(" msg_id={}", id)).unwrap_or_default();
    let prefixed = format!("[{}: {}{}] {}",
        capitalize(&msg.service), msg.user_id, msg_id_suffix, body);

    if agent_type.is_none() {
        let preview: String = prefixed.chars().take(120).collect();
        log::info!(
            "[bridge] queued inbound for pane={} (agent restarting): {}",
            pane_id, preview,
        );
        super::inbound_buffer::push(&pane_id, prefixed);
        return Ok(());
    }

    let pty_id = svc.resolve_pty_id(&pane_id)?;
    let paste = format!("\x1b[200~{}\x1b[201~", prefixed);
    svc.pty.lock().write(&pty_id, paste.as_bytes()).map_err(|e| e.to_string())?;

    // Send Enter with retry — Claude TUI needs time to process paste
    let pty = svc.pty.clone();
    let pty_id = pty_id.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(500));
        let _ = pty.lock().write(&pty_id, b"\r");
        std::thread::sleep(std::time::Duration::from_millis(2000));
        let _ = pty.lock().write(&pty_id, b"\r");
    });

    Ok(())
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn test_create_outbound_channel() {
        let (sender, _rx) = create_outbound_channel();
        assert!(sender.lock().is_some());
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
        }
    }

    async fn spawn_transcribe_server(
        text: &'static str,
    ) -> (std::net::SocketAddr, tokio::sync::oneshot::Sender<()>, tokio::task::JoinHandle<()>) {
        use axum::{routing::{get, post}, Json, Router};
        let router = Router::new()
            .route("/healthz", get(|| async { "ok" }))
            .route("/transcribe", post(move || async move {
                Json(serde_json::json!({ "text": text }))
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async move { let _ = rx.await; })
                .await;
        });
        (addr, tx, handle)
    }

    #[tokio::test]
    async fn body_includes_transcript_and_file_uri() {
        voice_transcribe::reset_health_cache();
        let (addr, stop, handle) = spawn_transcribe_server("привет мир").await;
        let cfg = enabled_cfg(&format!("http://{}", addr));

        let body = build_message_body(&voice_msg("/tmp/voice.oga"), &cfg).await;
        assert!(body.contains("(voice transcript: \"привет мир\")"), "body: {}", body);
        assert!(body.contains("file:///tmp/voice.oga"), "body: {}", body);

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

        let body = build_message_body(&voice_msg("/tmp/voice.oga"), &cfg).await;
        assert!(!body.contains("voice transcript"), "should not have transcript: {}", body);
        assert_eq!(body, "[voice] file:///tmp/voice.oga");
    }

    #[tokio::test]
    async fn disabled_cfg_preserves_legacy_format() {
        voice_transcribe::reset_health_cache();
        let mut cfg = enabled_cfg("http://127.0.0.1:1");
        cfg.enabled = false;

        let body = build_message_body(&voice_msg("/tmp/voice.oga"), &cfg).await;
        assert_eq!(body, "[voice] file:///tmp/voice.oga");
    }

    #[tokio::test]
    async fn non_voice_attachments_not_transcribed() {
        voice_transcribe::reset_health_cache();
        // Daemon URL points nowhere; if we accidentally hit it, the test fails
        // because we'd see fallback path. With photo kind we never call it.
        let cfg = enabled_cfg("http://127.0.0.1:1");

        let body = build_message_body(&photo_msg("/tmp/pic.jpg"), &cfg).await;
        assert_eq!(body, "[photo: pic.jpg] file:///tmp/pic.jpg");
    }

    #[tokio::test]
    async fn empty_attachment_path_skips_transcription() {
        voice_transcribe::reset_health_cache();
        let cfg = enabled_cfg("http://127.0.0.1:1");

        let body = build_message_body(&voice_msg(""), &cfg).await;
        assert_eq!(body, "[voice] file://");
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

        let body = build_message_body(&msg, &cfg).await;
        let occurrences = body.matches("(voice transcript: \"ok\")").count();
        assert_eq!(occurrences, 2, "body: {}", body);
        assert!(body.contains("/a.oga"));
        assert!(body.contains("/b.oga"));

        let _ = stop.send(());
        let _ = handle.await;
    }

    #[tokio::test]
    async fn malformed_url_falls_back_to_raw() {
        voice_transcribe::reset_health_cache();
        let mut cfg = enabled_cfg("not-a-url");
        cfg.daemon_url = "not a url".into();

        let body = build_message_body(&voice_msg("/tmp/voice.oga"), &cfg).await;
        assert_eq!(body, "[voice] file:///tmp/voice.oga");
    }

    #[test]
    fn test_reply_markup_serialization() {
        use crate::bridge::traits::{InlineButton, InlineKeyboard};
        let kb: InlineKeyboard = vec![vec![
            InlineButton { text: "Yes".into(), data: Some("yes".into()), url: None },
            InlineButton { text: "No".into(), data: Some("no".into()), url: None },
        ]];
        let payload = serde_json::json!({ "inline_keyboard": kb });
        let s = payload.to_string();
        assert!(s.contains("\"callback_data\""), "should serialize as callback_data: {}", s);
        assert!(!s.contains("\"data\""), "should not contain raw 'data' key: {}", s);
        let rows = payload["inline_keyboard"].as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].as_array().unwrap().len(), 2);
    }

    use std::sync::Arc;
    use parking_lot::Mutex as PLMutex;

    async fn spawn_fake_telegram(captured: Arc<PLMutex<Vec<serde_json::Value>>>) -> String {
        use axum::{routing::post, Router, Json};

        let captured_a = captured.clone();
        let captured_b = captured.clone();

        let app = Router::new()
            .route("/bot:method", post(move |Json(body): Json<serde_json::Value>| {
                let captured = captured_a.clone();
                async move {
                    captured.lock().push(body);
                    Json(serde_json::json!({"ok": true, "result": {"message_id": 42}}))
                }
            }))
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
        assert_eq!(recorded.len(), 1, "must NOT trigger fallback retry, got {} requests", recorded.len());
        let body = &recorded[0];
        assert_eq!(body.get("parse_mode").and_then(|v| v.as_str()), Some("HTML"),
            "first request must be HTML mode (cold-path fallback only)");
        let text = body.get("text").and_then(|v| v.as_str()).unwrap();
        assert!(!text.contains("<date>"), "raw <date> must be sanitized; got: {}", text);
        assert!(text.contains("&lt;date&gt;"), "must contain escaped &lt;date&gt;; got: {}", text);
        assert!(text.contains("<code>"), "must keep <code> wrapper; got: {}", text);
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
}
