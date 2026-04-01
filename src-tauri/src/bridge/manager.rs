//! BridgeManager v4 — CLI-based bridge between agents and Telegram.
//!
//! Outbound: agents call `codeterm bridge send` CLI → socket → channel → Telegram API.
//! Inbound: receives messages from Telegram, injects into PTY with [Telegram: user_id] prefix.

use std::sync::Arc;
use tokio::sync::mpsc;
use parking_lot::Mutex;

use super::traits::{InboundMessage, MessagingService, ReplyContext};
use crate::service::general_agent;
use crate::service::ServiceContext;

/// A complete outbound message to send via Telegram.
#[derive(Debug, Clone)]
pub struct OutboundTelegramMessage {
    pub user_id: Option<i64>,
    pub text: String,
}

/// Shared sender for outbound messages — socket/REST bridge.send writes here.
pub type OutboundSender = Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<OutboundTelegramMessage>>>>;

/// Create a new outbound channel pair. Returns (sender, receiver).
pub fn create_outbound_channel() -> (OutboundSender, tokio::sync::mpsc::UnboundedReceiver<OutboundTelegramMessage>) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (Arc::new(Mutex::new(Some(tx))), rx)
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

        // Start each service's polling loop
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
            .build().unwrap_or_else(|_| reqwest::Client::new());

        // Drop original tx so rx closes when all service tasks die
        drop(tx);

        log::info!("[bridge] manager v3 started (channel mode)");

        // Run inbound and outbound concurrently
        let fallback_clone = fallback_chat_id.clone();
        let svc = self.svc.clone();
        let client_clone = client.clone();
        let token_clone = token.clone();

        loop {
            tokio::select! {
                // Inbound: Telegram → Agent PTY
                Some(msg) = rx.recv() => {
                    log::info!("[bridge] inbound from {} ({}): {}",
                        msg.user_name, msg.service, &msg.text[..msg.text.len().min(100)]);

                    if let ReplyContext::Telegram { chat_id } = &msg.reply_ctx {
                        *fallback_clone.lock() = Some(*chat_id);
                    }

                    if let Err(e) = inject_message(&svc, &msg) {
                        log::error!("[bridge] inject failed: {}", e);
                    }
                }

                // Outbound: marker channel → Telegram
                Some(out_msg) = outbound_rx.recv() => {
                    let chat_id = out_msg.user_id
                        .or_else(|| *fallback_clone.lock())
                        .unwrap_or(0);

                    if chat_id == 0 {
                        log::warn!("[bridge-outbound] no chat_id, dropping message");
                        continue;
                    }

                    log::info!("[bridge-outbound] sending {} chars to {}", out_msg.text.len(), chat_id);

                    for chunk in super::response::chunk_message(&out_msg.text, super::telegram::TELEGRAM_MAX_LEN) {
                        let _ = client_clone
                            .post(format!("https://api.telegram.org/bot{}/sendMessage", token_clone))
                            .json(&serde_json::json!({
                                "chat_id": chat_id,
                                "text": chunk,
                                "disable_web_page_preview": true,
                            }))
                            .timeout(std::time::Duration::from_secs(10))
                            .send().await;
                    }
                }

                else => {
                    log::error!("[bridge] all channels closed — this should not happen");
                    // Sleep and retry rather than dying
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                }
            }
        }

        log::warn!("[bridge] manager stopped");
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
