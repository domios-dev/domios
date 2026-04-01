//! BridgeManager v3 — channel-based bridge between General Agent PTY and Telegram.
//!
//! Outbound: process_pty_output detects |-|TELEGRAM_SEND/END|-| markers, sends to channel.
//!           BridgeManager reads from channel and sends to Telegram. No raw buffer scanning.
//! Inbound: receives messages from Telegram, injects into PTY with [Telegram: user_id] prefix.

use std::sync::Arc;
use tokio::sync::mpsc;
use parking_lot::Mutex;

use super::traits::{InboundMessage, MessagingService, ReplyContext};
use crate::service::general_agent;
use crate::service::ServiceContext;

/// Marker constants — agent wraps Telegram responses in these.
pub const SEND_MARKER_PREFIX: &str = "|-|TELEGRAM_SEND:";
pub const END_MARKER: &str = "|-|TELEGRAM_END|-|";

/// A complete outbound message extracted from markers.
#[derive(Debug, Clone)]
pub struct OutboundTelegramMessage {
    pub user_id: Option<i64>,
    pub text: String,
}

/// Shared sender for outbound messages — process_pty_output sends here.
pub type OutboundSender = Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<OutboundTelegramMessage>>>>;

/// Create a new outbound channel pair.
/// Create outbound channel with delayed activation.
/// Returns (sender, receiver, activation_time).
/// Markers detected before activation_time are ignored to skip PTY output replay.
pub fn create_outbound_channel() -> (OutboundSender, tokio::sync::mpsc::UnboundedReceiver<OutboundTelegramMessage>, std::time::Instant) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let activate_at = std::time::Instant::now() + std::time::Duration::from_secs(20);
    (Arc::new(Mutex::new(Some(tx))), rx, activate_at)
}

/// Shared activation timestamp — markers before this time are dropped.
pub type ActivationTime = Arc<Mutex<std::time::Instant>>;

pub fn create_activation_time(at: std::time::Instant) -> ActivationTime {
    Arc::new(Mutex::new(at))
}

/// State machine for accumulating text between SEND and END markers.
/// Lives in process_pty_output context — called on every PTY chunk.
pub struct MarkerAccumulator {
    capturing: bool,
    buffer: String,
    user_id: Option<i64>,
    /// Recently sent message hashes to prevent duplicates on restart/replay.
    sent_hashes: std::collections::HashSet<u64>,
}

impl MarkerAccumulator {
    pub fn new() -> Self {
        Self { capturing: false, buffer: String::new(), user_id: None, sent_hashes: std::collections::HashSet::new() }
    }

    fn hash_message(text: &str, user_id: Option<i64>) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hasher);
        user_id.hash(&mut hasher);
        hasher.finish()
    }

    /// Feed a line of clean (ANSI-stripped) text. Returns a complete message when END marker found.
    pub fn feed_line(&mut self, line: &str) -> Option<OutboundTelegramMessage> {
        let trimmed = line.trim();

        if !self.capturing {
            // Look for SEND marker anywhere in the line (Claude TUI may prepend bullets/icons)
            if let Some(pos) = trimmed.find(SEND_MARKER_PREFIX) {
                let rest = &trimmed[pos + SEND_MARKER_PREFIX.len()..];
                let rest = rest.trim().trim_end_matches("|-|").trim();
                self.user_id = rest.parse::<i64>().ok();
                self.capturing = true;
                self.buffer.clear();
            }
            return None;
        }

        // We're capturing — check for END marker
        if trimmed.contains(END_MARKER) {
            self.capturing = false;
            let text = self.buffer.trim().to_string();
            if text.is_empty() { return None; }
            // Dedup: skip if we already sent this exact message
            let hash = Self::hash_message(&text, self.user_id);
            if self.sent_hashes.contains(&hash) {
                log::debug!("[bridge-detect] duplicate marker skipped: {}", &text[..text.len().min(50)]);
                return None;
            }
            self.sent_hashes.insert(hash);
            // Cap dedup cache at 100 entries
            if self.sent_hashes.len() > 100 { self.sent_hashes.clear(); }
            return Some(OutboundTelegramMessage {
                user_id: self.user_id.take(),
                text,
            });
        }

        // Accumulate line
        if !self.buffer.is_empty() { self.buffer.push('\n'); }
        self.buffer.push_str(trimmed);
        None
    }
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
    fn test_marker_accumulator_basic() {
        let mut acc = MarkerAccumulator::new();
        assert!(acc.feed_line("some output").is_none());
        assert!(acc.feed_line("|-|TELEGRAM_SEND: 186809246|-|").is_none());
        assert!(acc.feed_line("Hello from agent!").is_none());
        let msg = acc.feed_line("|-|TELEGRAM_END|-|").unwrap();
        assert_eq!(msg.user_id, Some(186809246));
        assert_eq!(msg.text, "Hello from agent!");
    }

    #[test]
    fn test_marker_accumulator_multiline() {
        let mut acc = MarkerAccumulator::new();
        acc.feed_line("|-|TELEGRAM_SEND: 123|-|");
        acc.feed_line("Line 1");
        acc.feed_line("Line 2");
        let msg = acc.feed_line("|-|TELEGRAM_END|-|").unwrap();
        assert_eq!(msg.text, "Line 1\nLine 2");
    }

    #[test]
    fn test_marker_accumulator_ignores_non_markers() {
        let mut acc = MarkerAccumulator::new();
        assert!(acc.feed_line("random text").is_none());
        assert!(acc.feed_line("more text").is_none());
        // No message produced
    }

    #[test]
    fn test_marker_accumulator_empty_body() {
        let mut acc = MarkerAccumulator::new();
        acc.feed_line("|-|TELEGRAM_SEND: 123|-|");
        assert!(acc.feed_line("|-|TELEGRAM_END|-|").is_none()); // empty = skip
    }

    #[test]
    fn test_capitalize() {
        assert_eq!(capitalize("telegram"), "Telegram");
        assert_eq!(capitalize(""), "");
    }
}
