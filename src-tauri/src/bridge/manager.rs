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

use super::traits::{InboundMessage, MessagingService, ReplyContext};
use crate::service::general_agent;
use crate::service::ServiceContext;

/// Auto-edit window — if a new send arrives within this time, edit the previous message.
const AUTO_EDIT_WINDOW_SECS: u64 = 60;

#[derive(Debug, Clone)]
pub struct OutboundTelegramMessage {
    pub user_id: Option<i64>,
    pub text: String,
    /// If true, append text to the previous message instead of replacing.
    pub append: bool,
}

pub type OutboundSender = Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<OutboundTelegramMessage>>>>;

pub fn create_outbound_channel() -> (OutboundSender, tokio::sync::mpsc::UnboundedReceiver<OutboundTelegramMessage>) {
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
        mut outbound_rx: tokio::sync::mpsc::UnboundedReceiver<OutboundTelegramMessage>,
    ) {
        let (tx, mut rx) = mpsc::channel::<InboundMessage>(64);
        let fallback_chat_id: Arc<Mutex<Option<i64>>> = Arc::new(Mutex::new(None));

        for service in services {
            let tx_clone = tx.clone();
            let id = service.id().to_string();
            tokio::spawn(async move {
                log::info!("[bridge] starting service: {}", id);
                service.start(tx_clone).await;
                log::warn!("[bridge] service stopped: {}", id);
            });
        }

        let token = super::telegram::read_token().unwrap_or_default();
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
                        .collect::<Vec<_>>()
                        .join("\n");
                    let first = &messages[0];

                    log::info!("[bridge] inbound from {} ({}, {} msg): {}",
                        first.user_name, first.service, messages.len(),
                        &merged_text[..merged_text.len().min(100)]);

                    if let ReplyContext::Telegram { chat_id } = &first.reply_ctx {
                        *fallback_clone.lock() = Some(*chat_id);
                        send_typing(&client, &token, *chat_id).await;
                    }

                    let merged_msg = InboundMessage {
                        service: first.service.clone(),
                        user_id: first.user_id,
                        user_name: first.user_name.clone(),
                        text: merged_text,
                        reply_ctx: first.reply_ctx.clone(),
                    };

                    if let Err(e) = inject_message(&svc, &merged_msg) {
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

                    log::info!("[bridge-outbound] {} {} chars to {}",
                        if out_msg.append { "appending" } else { "sending" }, out_msg.text.len(), chat_id);

                    // Build final text — append to previous or replace
                    let final_text = if out_msg.append {
                        if let Some(state) = chat_states.get(&chat_id) {
                            if state.sent_at.elapsed().as_secs() < AUTO_EDIT_WINDOW_SECS {
                                format!("{}\n{}", state.text, out_msg.text)
                            } else {
                                out_msg.text.clone()
                            }
                        } else {
                            out_msg.text.clone()
                        }
                    } else {
                        out_msg.text.clone()
                    };

                    // Check if we can edit recent message
                    let can_edit = chat_states.get(&chat_id)
                        .map(|s| s.sent_at.elapsed().as_secs() < AUTO_EDIT_WINDOW_SECS)
                        .unwrap_or(false);

                    if can_edit && final_text.len() <= super::telegram::TELEGRAM_MAX_LEN {
                        let msg_id = chat_states[&chat_id].message_id;
                        let edited = edit_message(&client, &token, chat_id, msg_id, &final_text).await;
                        if !edited {
                            if let Some(new_id) = send_message(&client, &token, chat_id, &final_text).await {
                                chat_states.insert(chat_id, ChatState { message_id: new_id, sent_at: Instant::now(), text: final_text });
                            }
                        } else {
                            if let Some(state) = chat_states.get_mut(&chat_id) {
                                state.sent_at = Instant::now();
                                state.text = final_text;
                            }
                        }
                    } else {
                        // Send as new message(s)
                        let chunks: Vec<String> = super::response::chunk_message(&final_text, super::telegram::TELEGRAM_MAX_LEN);
                        let mut last_id = None;
                        for chunk in &chunks {
                            last_id = send_message(&client, &token, chat_id, chunk).await;
                        }
                        if let Some(id) = last_id {
                            chat_states.insert(chat_id, ChatState { message_id: id, sent_at: Instant::now(), text: final_text });
                        }
                    }
                }

                else => {
                    log::error!("[bridge] all channels closed");
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                }
            }
        }
    }
}

/// Send typing indicator to chat.
async fn send_typing(client: &reqwest::Client, token: &str, chat_id: i64) {
    let _ = client
        .post(format!("https://api.telegram.org/bot{}/sendChatAction", token))
        .json(&serde_json::json!({ "chat_id": chat_id, "action": "typing" }))
        .timeout(std::time::Duration::from_secs(5))
        .send().await;
}

/// Send a new message. Returns message_id on success.
async fn send_message(client: &reqwest::Client, token: &str, chat_id: i64, text: &str) -> Option<i64> {
    let resp = client
        .post(format!("https://api.telegram.org/bot{}/sendMessage", token))
        .json(&serde_json::json!({
            "chat_id": chat_id,
            "text": text,
            "parse_mode": "Markdown",
            "disable_web_page_preview": true,
        }))
        .timeout(std::time::Duration::from_secs(10))
        .send().await.ok()?;

    let body: serde_json::Value = resp.json().await.ok()?;

    // If Markdown parsing failed, retry without parse_mode
    if body.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let resp2 = client
            .post(format!("https://api.telegram.org/bot{}/sendMessage", token))
            .json(&serde_json::json!({
                "chat_id": chat_id,
                "text": text,
                "disable_web_page_preview": true,
            }))
            .timeout(std::time::Duration::from_secs(10))
            .send().await.ok()?;
        let body2: serde_json::Value = resp2.json().await.ok()?;
        return body2.get("result").and_then(|r| r.get("message_id")).and_then(|v| v.as_i64());
    }

    body.get("result").and_then(|r| r.get("message_id")).and_then(|v| v.as_i64())
}

/// Edit an existing message. Returns true on success.
async fn edit_message(client: &reqwest::Client, token: &str, chat_id: i64, message_id: i64, text: &str) -> bool {
    let resp = client
        .post(format!("https://api.telegram.org/bot{}/editMessageText", token))
        .json(&serde_json::json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "text": text,
            "parse_mode": "Markdown",
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
            // "message is not modified" is ok (idempotent)
            let desc = body.get("description").and_then(|v| v.as_str()).unwrap_or("");
            if desc.contains("message is not modified") {
                return true;
            }
            // Retry without Markdown if parse failed
            if desc.contains("parse") {
                let resp2 = client
                    .post(format!("https://api.telegram.org/bot{}/editMessageText", token))
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

fn inject_message(svc: &ServiceContext, msg: &InboundMessage) -> Result<(), String> {
    let pane_id = {
        let mux = svc.mux.lock();
        mux.resolve_pane_id(general_agent::GENERAL_ALIAS)
            .ok_or("General Agent not found")?
    };

    let prefixed = format!("[{}: {}] {}",
        capitalize(&msg.service), msg.user_id, msg.text);

    // Bracket paste the text
    let pty_id = svc.resolve_pty_id(&pane_id)?;
    let paste = format!("\x1b[200~{}\x1b[201~", prefixed);
    svc.pty.lock().write(&pty_id, paste.as_bytes()).map_err(|e| e.to_string())?;

    // Send Enter with retry — Claude TUI needs time to process paste
    let pty = svc.pty.clone();
    let pty_id = pty_id.clone();
    std::thread::spawn(move || {
        // First attempt after 500ms
        std::thread::sleep(std::time::Duration::from_millis(500));
        let _ = pty.lock().write(&pty_id, b"\r");
        // Retry after 2s if first didn't take
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
    fn test_capitalize() {
        assert_eq!(capitalize("telegram"), "Telegram");
        assert_eq!(capitalize(""), "");
    }

    #[test]
    fn test_create_outbound_channel() {
        let (sender, _rx) = create_outbound_channel();
        assert!(sender.lock().is_some());
    }
}
