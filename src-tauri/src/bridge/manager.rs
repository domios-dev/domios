//! BridgeManager v2 — marker-based bridge between General Agent PTY and external services.
//!
//! Outbound: watches PTY output for |-|TELEGRAM_SEND: USER_ID|-| ... |-|TELEGRAM_END|-| markers.
//! Inbound: receives messages from services, injects into PTY with [Service: user_id] prefix.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::mpsc;

use super::traits::{InboundMessage, MessagingService, ReplyContext};
use crate::service::general_agent;
use crate::service::ServiceContext;

/// How often to poll OutputBuffer for new markers (ms).
const POLL_MS: u64 = 300;

/// Marker constants.
const SEND_MARKER: &str = "|-|TELEGRAM_SEND:";
const END_MARKER: &str = "|-|TELEGRAM_END|-|";

struct OutboundState {
    /// Fallback chat_id from last inbound message (used if marker doesn't specify).
    chat_id: parking_lot::Mutex<Option<i64>>,
    /// Last scanned offset into full output buffer.
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

        // Start outbound watcher (Agent → Telegram via markers)
        {
            let svc = self.svc.clone();
            let out = outbound.clone();
            tokio::spawn(async move {
                outbound_loop(svc, out).await;
            });
        }

        log::info!("[bridge] manager v2 started (marker mode)");

        // Inbound loop (Telegram → Agent)
        while let Some(msg) = rx.recv().await {
            log::info!("[bridge] inbound from {} ({}): {}",
                msg.user_name, msg.service, &msg.text[..msg.text.len().min(100)]);

            if let ReplyContext::Telegram { chat_id } = &msg.reply_ctx {
                *outbound.chat_id.lock() = Some(*chat_id);
            }

            if let Err(e) = self.inject_message(&msg) {
                log::error!("[bridge] inject failed: {}", e);
            }
        }
    }

    fn inject_message(&self, msg: &InboundMessage) -> Result<(), String> {
        let pty_id = {
            let mux = self.svc.mux.lock();
            let pid = mux.resolve_pane_id(general_agent::GENERAL_ALIAS)
                .ok_or("General Agent not found")?;
            let pane = mux.get_pane(&pid).ok_or("General Agent pane missing")?;
            pane.pty_id.clone().ok_or("General Agent PTY not ready")?
        };

        let prefixed = format!("[{}: {}] {}\r",
            capitalize(&msg.service), msg.user_id, msg.text);

        self.svc.pty.lock().write(&pty_id, prefixed.as_bytes())
    }
}

/// Outbound loop — watches raw output buffer for TELEGRAM_SEND/TELEGRAM_END markers.
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

    let pty_id = loop {
        let id = {
            let mux = svc.mux.lock();
            mux.resolve_pane_id(general_agent::GENERAL_ALIAS)
                .and_then(|pid| mux.get_pane(&pid).and_then(|p| p.pty_id.clone()))
        };
        if let Some(id) = id { break id; }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    };

    // Start scanning from current position
    let initial = svc.output_buf.lock()
        .get_full(&pty_id).map(|b| b.len()).unwrap_or(0);
    state.offset.store(initial, Ordering::Relaxed);

    log::info!("[bridge-outbound] marker watcher started for PTY {}", &pty_id[..8.min(pty_id.len())]);

    loop {
        tokio::time::sleep(std::time::Duration::from_millis(POLL_MS)).await;

        let offset = state.offset.load(Ordering::Relaxed);
        let raw = {
            let buf = svc.output_buf.lock();
            buf.get_full(&pty_id).and_then(|b| {
                if b.len() > offset {
                    Some(String::from_utf8_lossy(&b[offset..]).to_string())
                } else {
                    None
                }
            })
        };

        let Some(raw) = raw else { continue };

        // Look for complete marker pairs
        let clean = super::response::strip_ansi(&raw);
        let Some(send_pos) = clean.find(SEND_MARKER) else { continue };
        let Some(end_pos) = clean.find(END_MARKER) else { continue };
        if end_pos <= send_pos { continue; }

        // Extract the content between markers
        let header_end = send_pos + SEND_MARKER.len();
        let after_header = &clean[header_end..end_pos];

        // Parse user_id from header: "|-|TELEGRAM_SEND: 186809246|-|"
        let (user_id_str, body) = if let Some(marker_close) = after_header.find("|-|") {
            let uid = after_header[..marker_close].trim();
            let content = after_header[marker_close + 3..].trim();
            (uid, content)
        } else {
            ("", after_header.trim())
        };

        if body.is_empty() {
            // Advance offset past end marker
            let advance = raw.find("|-|TELEGRAM_END|-|")
                .map(|p| p + END_MARKER.len())
                .unwrap_or(raw.len());
            state.offset.store(offset + advance, Ordering::Relaxed);
            continue;
        }

        // Determine chat_id
        let chat_id = if let Ok(uid) = user_id_str.parse::<i64>() {
            uid
        } else {
            match *state.chat_id.lock() {
                Some(id) => id,
                None => {
                    log::warn!("[bridge-outbound] no chat_id, skipping");
                    continue;
                }
            }
        };

        log::info!("[bridge-outbound] sending {} chars to chat {}", body.len(), chat_id);

        // Send to Telegram
        for chunk in super::response::chunk_message(body, super::telegram::TELEGRAM_MAX_LEN) {
            let _ = client
                .post(format!("https://api.telegram.org/bot{}/sendMessage", token))
                .json(&serde_json::json!({
                    "chat_id": chat_id,
                    "text": chunk,
                    "disable_web_page_preview": true,
                }))
                .timeout(std::time::Duration::from_secs(10))
                .send().await;
        }

        // Advance offset past end marker
        let advance = raw.find("|-|TELEGRAM_END|-|")
            .map(|p| p + END_MARKER.len())
            .unwrap_or(raw.len());
        state.offset.store(offset + advance, Ordering::Relaxed);
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
        assert_eq!(capitalize(""), "");
    }

    #[test]
    fn test_marker_parsing() {
        let text = "some output\n|-|TELEGRAM_SEND: 186809246|-|\nHello from agent!\n|-|TELEGRAM_END|-|\nmore output";
        let send_pos = text.find(SEND_MARKER).unwrap();
        let end_pos = text.find(END_MARKER).unwrap();
        assert!(end_pos > send_pos);

        let header_end = send_pos + SEND_MARKER.len();
        let after_header = &text[header_end..end_pos];
        let marker_close = after_header.find("|-|").unwrap();
        let uid = after_header[..marker_close].trim();
        let body = after_header[marker_close + 3..].trim();

        assert_eq!(uid, "186809246");
        assert_eq!(body, "Hello from agent!");
    }
}
