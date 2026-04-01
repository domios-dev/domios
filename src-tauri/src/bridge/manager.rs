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
}

pub type OutboundSender = Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<OutboundTelegramMessage>>>>;

pub fn create_outbound_channel() -> (OutboundSender, tokio::sync::mpsc::UnboundedReceiver<OutboundTelegramMessage>) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (Arc::new(Mutex::new(Some(tx))), rx)
}

/// Track last sent message per chat for auto-edit.
struct ChatState {
    message_id: i64,
    sent_at: Instant,
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
                    log::info!("[bridge] inbound from {} ({}): {}",
                        msg.user_name, msg.service, &msg.text[..msg.text.len().min(100)]);

                    if let ReplyContext::Telegram { chat_id } = &msg.reply_ctx {
                        *fallback_clone.lock() = Some(*chat_id);
                        // Send typing indicator
                        send_typing(&client, &token, *chat_id).await;
                    }

                    if let Err(e) = inject_message(&svc, &msg) {
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

                    log::info!("[bridge-outbound] sending {} chars to {}", out_msg.text.len(), chat_id);

                    // Check if we can auto-edit recent message
                    let can_edit = chat_states.get(&chat_id)
                        .map(|s| s.sent_at.elapsed().as_secs() < AUTO_EDIT_WINDOW_SECS)
                        .unwrap_or(false);

                    if can_edit && out_msg.text.len() <= super::telegram::TELEGRAM_MAX_LEN {
                        let msg_id = chat_states[&chat_id].message_id;
                        let edited = edit_message(&client, &token, chat_id, msg_id, &out_msg.text).await;
                        if !edited {
                            // Edit failed — send as new message
                            if let Some(new_id) = send_message(&client, &token, chat_id, &out_msg.text).await {
                                chat_states.insert(chat_id, ChatState { message_id: new_id, sent_at: Instant::now() });
                            }
                        } else {
                            // Update timestamp
                            if let Some(state) = chat_states.get_mut(&chat_id) {
                                state.sent_at = Instant::now();
                            }
                        }
                    } else {
                        // Send as new message(s)
                        let chunks: Vec<String> = super::response::chunk_message(&out_msg.text, super::telegram::TELEGRAM_MAX_LEN);
                        let mut last_id = None;
                        for chunk in chunks {
                            last_id = send_message(&client, &token, chat_id, &chunk).await;
                        }
                        if let Some(id) = last_id {
                            chat_states.insert(chat_id, ChatState { message_id: id, sent_at: Instant::now() });
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
    let pty_id = {
        let mux = svc.mux.lock();
        let pid = mux.resolve_pane_id(general_agent::GENERAL_ALIAS)
            .ok_or("General Agent not found")?;
        let pane = mux.get_pane(&pid).ok_or("General Agent pane missing")?;
        pane.pty_id.clone().ok_or("General Agent PTY not ready")?
    };

    let prefixed = format!("[{}: {}] {}\r",
        capitalize(&msg.service), msg.user_id, msg.text);

    svc.pty.lock().write(&pty_id, prefixed.as_bytes())
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
