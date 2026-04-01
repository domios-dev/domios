//! BridgeManager v2 — streaming bridge between General Agent PTY and external services.
//!
//! Outbound: polls OutputBuffer for new content, strips ANSI, sends to Telegram.
//! Inbound: receives messages from services, injects into PTY with source prefix.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::mpsc;

use super::traits::{InboundMessage, MessagingService, ReplyContext};
use crate::service::general_agent;
use crate::service::ServiceContext;

/// How often to poll OutputBuffer for new agent output (ms).
const OUTBOUND_POLL_MS: u64 = 500;

/// Shared state for the outbound streamer — tracks which Telegram chat to stream to.
struct OutboundState {
    /// Chat ID to stream agent output to (set when first inbound message arrives).
    chat_id: parking_lot::Mutex<Option<i64>>,
    /// Last sent offset into the full output buffer.
    offset: AtomicUsize,
}

pub struct BridgeManager {
    svc: ServiceContext,
}

impl BridgeManager {
    pub fn new(svc: ServiceContext) -> Self {
        Self { svc }
    }

    pub async fn start(self, services: Vec<Box<dyn MessagingService>>) {
        let (tx, mut rx) = mpsc::channel::<InboundMessage>(64);

        for service in services {
            let tx_clone = tx.clone();
            let id = service.id().to_string();
            tokio::spawn(async move {
                log::info!("[bridge] starting service: {}", id);
                service.start(tx_clone).await;
            });
        }

        let outbound = Arc::new(OutboundState {
            chat_id: parking_lot::Mutex::new(None),
            offset: AtomicUsize::new(0),
        });

        // Start outbound streamer (Agent → Telegram)
        {
            let svc = self.svc.clone();
            let out = outbound.clone();
            tokio::spawn(async move {
                outbound_loop(svc, out).await;
            });
        }

        log::info!("[bridge] manager v2 started (streaming mode)");

        // Inbound loop (Telegram → Agent)
        while let Some(msg) = rx.recv().await {
            log::info!("[bridge] inbound from {} ({}): {}",
                msg.user_name, msg.service, &msg.text[..msg.text.len().min(100)]);

            // Remember chat_id for outbound streaming
            if let ReplyContext::Telegram { chat_id } = &msg.reply_ctx {
                *outbound.chat_id.lock() = Some(*chat_id);
            }

            // Inject message into PTY with source prefix
            if let Err(e) = self.inject_message(&msg) {
                log::error!("[bridge] inject failed: {}", e);
            }
        }
    }

    /// Inject an inbound message into the General Agent PTY with source prefix.
    fn inject_message(&self, msg: &InboundMessage) -> Result<(), String> {
        let pty_id = {
            let mux = self.svc.mux.lock();
            let pid = mux.resolve_pane_id(general_agent::GENERAL_ALIAS)
                .ok_or("General Agent not found")?;
            let pane = mux.get_pane(&pid).ok_or("General Agent pane missing")?;
            pane.pty_id.clone().ok_or("General Agent PTY not ready")?
        };

        // Format: [Telegram: 186809246] message text
        let prefixed = format!("[{}: {}] {}\r",
            capitalize(&msg.service), msg.user_id, msg.text);

        self.svc.pty.lock().write(&pty_id, prefixed.as_bytes())
    }
}

/// Outbound streaming loop — polls OutputBuffer and sends new content to Telegram.
async fn outbound_loop(svc: ServiceContext, state: Arc<OutboundState>) {
    let token = match super::telegram::read_token() {
        Some(t) => t,
        None => {
            log::warn!("[bridge-outbound] no Telegram token, outbound disabled");
            return;
        }
    };
    let client = reqwest::Client::new();

    // Wait for General Agent to start
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    // Find the PTY ID
    let pty_id = loop {
        let id = {
            let mux = svc.mux.lock();
            mux.resolve_pane_id(general_agent::GENERAL_ALIAS)
                .and_then(|pid| mux.get_pane(&pid).and_then(|p| p.pty_id.clone()))
        };
        if let Some(id) = id { break id; }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    };

    // Initialize offset to current buffer length (don't send historical output)
    let initial_offset = svc.output_buf.lock()
        .get_full(&pty_id).map(|b| b.len()).unwrap_or(0);
    state.offset.store(initial_offset, Ordering::Relaxed);

    log::info!("[bridge-outbound] streaming started for PTY {}, offset={}", &pty_id[..8.min(pty_id.len())], initial_offset);

    loop {
        tokio::time::sleep(std::time::Duration::from_millis(OUTBOUND_POLL_MS)).await;

        let chat_id = *state.chat_id.lock();
        let Some(chat_id) = chat_id else { continue };

        // Check if agent is idle (only send complete responses, not mid-stream)
        let status = svc.agent_tracker.lock().get_status(&pty_id);
        if status == crate::agent::status::PaneStatus::Working {
            continue; // Wait until agent finishes
        }

        let last_offset = state.offset.load(Ordering::Relaxed);
        let new_content = {
            let buf = svc.output_buf.lock();
            buf.get_full(&pty_id).and_then(|b| {
                if b.len() > last_offset {
                    Some(String::from_utf8_lossy(&b[last_offset..]).to_string())
                } else {
                    None
                }
            })
        };

        let Some(raw) = new_content else { continue };
        if raw.trim().is_empty() { continue; }

        let clean = super::response::strip_ansi(&raw);
        if clean.is_empty() { continue; }

        // Update offset
        let new_offset = svc.output_buf.lock()
            .get_full(&pty_id).map(|b| b.len()).unwrap_or(last_offset);
        state.offset.store(new_offset, Ordering::Relaxed);

        // Send to Telegram
        for chunk in super::response::chunk_message(&clean, super::telegram::TELEGRAM_MAX_LEN) {
            let resp = client
                .post(format!("https://api.telegram.org/bot{}/sendMessage", token))
                .json(&serde_json::json!({
                    "chat_id": chat_id,
                    "text": chunk,
                    "disable_web_page_preview": true,
                }))
                .timeout(std::time::Duration::from_secs(10))
                .send().await;

            if let Err(e) = resp {
                log::warn!("[bridge-outbound] send failed: {}", e);
            }
        }
    }
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
        assert_eq!(capitalize("discord"), "Discord");
        assert_eq!(capitalize(""), "");
    }
}
