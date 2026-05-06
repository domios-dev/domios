//! Telegram Bot API via reqwest — no extra crates.

use std::pin::Pin;
use std::future::Future;
use std::sync::Arc;

use tokio::sync::Notify;

use super::supervisor::BridgeStatusSink;
use super::traits::{InboundAttachment, InboundMessage, MessagingService, ReplyContext};

pub const TELEGRAM_MAX_LEN: usize = 4096;

/// Max total size of bridge-downloads dir before eviction (100 MB).
const DOWNLOADS_MAX_BYTES: u64 = 100 * 1024 * 1024;

pub fn token_path() -> std::path::PathBuf {
    crate::service::general_agent::secrets_dir().join("telegram.token")
}

pub fn read_token() -> Option<String> {
    std::fs::read_to_string(token_path()).ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Empty list = nobody allowed. Only explicitly listed IDs pass.
fn user_allowed(allowed: &[String], user_id: i64) -> bool {
    !allowed.is_empty() && allowed.contains(&user_id.to_string())
}

pub fn telegram_api_base() -> String {
    std::env::var("TELEGRAM_API_BASE").unwrap_or_else(|_| "https://api.telegram.org".to_string())
}

pub struct TelegramService {
    token: String,
    client: reqwest::Client,
    api_base: String,
}

impl TelegramService {
    pub fn new(token: String, _allowed_users: Vec<String>) -> Self {
        Self::with_base(token, telegram_api_base())
    }

    pub fn with_base(token: String, api_base: String) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(40)) // > Telegram's 30s long poll
            .tcp_keepalive(std::time::Duration::from_secs(15))
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { token, client, api_base }
    }

    fn api_url(&self, method: &str) -> String {
        format!("{}/bot{}/{}", self.api_base.trim_end_matches('/'), self.token, method)
    }

    fn is_user_allowed(&self, user_id: i64) -> bool {
        // Always read fresh config — CLI/UI may change allowed_users at any time.
        let allowed = &crate::config::load_config().integrations.telegram_allowed_users;
        user_allowed(allowed, user_id)
    }

    pub async fn send_message(&self, chat_id: i64, text: &str) -> Result<(), String> {
        for chunk in super::response::chunk_message(text, TELEGRAM_MAX_LEN) {
            let resp = self.client
                .post(&self.api_url("sendMessage"))
                .json(&serde_json::json!({ "chat_id": chat_id, "text": chunk, "disable_web_page_preview": true }))
                .timeout(std::time::Duration::from_secs(10))
                .send().await.map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                log::warn!("[telegram] sendMessage failed: {}", resp.text().await.unwrap_or_default());
            }
        }
        Ok(())
    }

    /// Download a file from Telegram by file_id. Returns local path on success.
    async fn download_file(&self, file_id: &str, downloads_dir: &std::path::Path, filename_hint: Option<&str>) -> Option<String> {
        // Step 1: getFile to get file_path
        let resp = self.client
            .post(&self.api_url("getFile"))
            .json(&serde_json::json!({ "file_id": file_id }))
            .timeout(std::time::Duration::from_secs(10))
            .send().await.ok()?;
        let body: serde_json::Value = resp.json().await.ok()?;
        let tg_path = body.get("result")?.get("file_path")?.as_str()?;

        // Step 2: download from <api_base>/file/bot<token>/<file_path>
        let url = format!("{}/file/bot{}/{}", self.api_base.trim_end_matches('/'), self.token, tg_path);
        let resp = self.client.get(&url)
            .timeout(std::time::Duration::from_secs(30))
            .send().await.ok()?;
        if !resp.status().is_success() {
            log::warn!("[telegram] download failed for {}: {}", file_id, resp.status());
            return None;
        }
        let bytes = resp.bytes().await.ok()?;

        // Determine local filename
        let ext = std::path::Path::new(tg_path).extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        let local_name = filename_hint
            .map(|n| n.to_string())
            .unwrap_or_else(|| format!("{}{}", file_id.chars().take(20).collect::<String>(), ext));
        let local_path = downloads_dir.join(&local_name);

        if let Err(e) = tokio::fs::write(&local_path, &bytes).await {
            log::error!("[telegram] failed to save file: {}", e);
            return None;
        }
        log::info!("[telegram] downloaded {} ({} bytes) -> {}", file_id, bytes.len(), local_path.display());

        // Evict old files if total size exceeds limit
        evict_downloads(downloads_dir);

        Some(local_path.to_string_lossy().to_string())
    }

    /// Extract a media attachment from a Telegram message, downloading the file.
    async fn extract_attachment(&self, msg: &serde_json::Value, downloads_dir: &std::path::Path) -> Option<InboundAttachment> {
        // Photo — array of sizes, pick largest (last)
        if let Some(photos) = msg.get("photo").and_then(|v| v.as_array()) {
            let best = photos.last()?;
            let file_id = best.get("file_id")?.as_str()?;
            let path = self.download_file(file_id, downloads_dir, None).await?;
            return Some(InboundAttachment { path, kind: "photo".into(), filename: None, mime_type: Some("image/jpeg".into()) });
        }
        // Document
        if let Some(doc) = msg.get("document") {
            let file_id = doc.get("file_id")?.as_str()?;
            let filename = doc.get("file_name").and_then(|v| v.as_str()).map(|s| s.to_string());
            let mime = doc.get("mime_type").and_then(|v| v.as_str()).map(|s| s.to_string());
            let path = self.download_file(file_id, downloads_dir, filename.as_deref()).await?;
            return Some(InboundAttachment { path, kind: "document".into(), filename, mime_type: mime });
        }
        // Voice
        if let Some(voice) = msg.get("voice") {
            let file_id = voice.get("file_id")?.as_str()?;
            let mime = voice.get("mime_type").and_then(|v| v.as_str()).map(|s| s.to_string());
            let path = self.download_file(file_id, downloads_dir, None).await?;
            return Some(InboundAttachment { path, kind: "voice".into(), filename: None, mime_type: mime });
        }
        // Video
        if let Some(video) = msg.get("video") {
            let file_id = video.get("file_id")?.as_str()?;
            let mime = video.get("mime_type").and_then(|v| v.as_str()).map(|s| s.to_string());
            let filename = video.get("file_name").and_then(|v| v.as_str()).map(|s| s.to_string());
            let path = self.download_file(file_id, downloads_dir, filename.as_deref()).await?;
            return Some(InboundAttachment { path, kind: "video".into(), filename, mime_type: mime });
        }
        // Animation (GIF)
        if let Some(anim) = msg.get("animation") {
            let file_id = anim.get("file_id")?.as_str()?;
            let mime = anim.get("mime_type").and_then(|v| v.as_str()).map(|s| s.to_string());
            let path = self.download_file(file_id, downloads_dir, None).await?;
            return Some(InboundAttachment { path, kind: "animation".into(), filename: None, mime_type: mime });
        }
        // Sticker
        if let Some(sticker) = msg.get("sticker") {
            let file_id = sticker.get("file_id")?.as_str()?;
            let path = self.download_file(file_id, downloads_dir, None).await?;
            return Some(InboundAttachment { path, kind: "sticker".into(), filename: None, mime_type: Some("image/webp".into()) });
        }
        // Video note (round video)
        if let Some(vnote) = msg.get("video_note") {
            let file_id = vnote.get("file_id")?.as_str()?;
            let path = self.download_file(file_id, downloads_dir, None).await?;
            return Some(InboundAttachment { path, kind: "video_note".into(), filename: None, mime_type: Some("video/mp4".into()) });
        }
        None
    }

    async fn poll_updates(
        self: Box<Self>,
        tx: tokio::sync::mpsc::Sender<InboundMessage>,
        cancel: Arc<Notify>,
        status: Arc<dyn BridgeStatusSink>,
    ) {
        let mut offset: i64 = 0;
        let mut error_count: u32 = 0;
        let mut poll_count: u64 = 0;

        // Create downloads directory for received media
        let downloads_dir = crate::service::general_agent::secrets_dir()
            .parent().unwrap_or(std::path::Path::new("."))
            .join("bridge-downloads");
        if let Err(e) = std::fs::create_dir_all(&downloads_dir) {
            log::error!("[telegram] failed to create downloads dir: {}", e);
        }

        loop {
            poll_count += 1;
            if poll_count % 10 == 1 {
                log::info!("[telegram] poll #{}, offset={}, errors={}", poll_count, offset, error_count);
            }

            let request = self.client
                .post(self.api_url("getUpdates"))
                .json(&serde_json::json!({ "offset": offset, "timeout": 30, "allowed_updates": ["message", "callback_query"] }))
                .send();

            let resp = tokio::select! {
                biased;
                _ = cancel.notified() => {
                    log::info!("[telegram] received stop signal; releasing getUpdates lock");
                    return;
                }
                r = request => r,
            };

            let body = match resp {
                Ok(r) if r.status().is_success() => {
                    error_count = 0;
                    match r.json::<serde_json::Value>().await {
                        Ok(v) => {
                            status.mark_poll_ok();
                            v
                        }
                        Err(e) => {
                            log::warn!("[telegram] parse error: {}", e);
                            status.mark_poll_error(&format!("parse error: {}", e));
                            if cancellable_sleep(&cancel, std::time::Duration::from_secs(1)).await { return; }
                            continue;
                        }
                    }
                }
                Ok(r) if r.status().as_u16() == 429 => {
                    let wait = 30u64; // Telegram rate limit
                    log::warn!("[telegram] rate limited, waiting {}s", wait);
                    status.mark_poll_error("429 Too Many Requests");
                    if cancellable_sleep(&cancel, std::time::Duration::from_secs(wait)).await { return; }
                    continue;
                }
                Ok(r) if r.status().as_u16() == 409 => {
                    error_count = error_count.saturating_add(1);
                    log::warn!("[telegram] 409 Conflict — another getUpdates poller still active");
                    status.mark_poll_error("409 Conflict");
                    let backoff = (2u64.pow(error_count.min(6))).min(60);
                    if cancellable_sleep(&cancel, std::time::Duration::from_secs(backoff)).await { return; }
                    continue;
                }
                Ok(r) => {
                    error_count += 1;
                    let http_status = r.status();
                    log::warn!("[telegram] API error {}", http_status);
                    status.mark_poll_error(&format!("HTTP {}", http_status));
                    let backoff = (2u64.pow(error_count.min(6))).min(60);
                    if cancellable_sleep(&cancel, std::time::Duration::from_secs(backoff)).await { return; }
                    continue;
                }
                Err(e) => {
                    error_count += 1;
                    log::warn!("[telegram] poll error (count={}): {}", error_count, e);
                    status.mark_poll_error(&e.to_string());
                    let backoff = (2u64.pow(error_count.min(6))).min(60);
                    if cancellable_sleep(&cancel, std::time::Duration::from_secs(backoff)).await { return; }
                    continue;
                }
            };

            let Some(updates) = body.get("result").and_then(|v| v.as_array()) else {
                if cancellable_sleep(&cancel, std::time::Duration::from_secs(1)).await { return; }
                continue;
            };

            for update in updates {
                if let Some(uid) = update.get("update_id").and_then(|v| v.as_i64()) { offset = uid + 1; }

                // Handle callback_query (inline button press)
                if let Some(cb) = update.get("callback_query") {
                    let cb_id = cb.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    let data = cb.get("data").and_then(|v| v.as_str()).unwrap_or("");
                    let user_id = cb.get("from").and_then(|f| f.get("id")).and_then(|v| v.as_i64()).unwrap_or(0);
                    let user_name = cb.get("from").and_then(|f| f.get("first_name").or_else(|| f.get("username"))).and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                    let chat_id = cb.get("message").and_then(|m| m.get("chat")).and_then(|c| c.get("id")).and_then(|v| v.as_i64()).unwrap_or(0);
                    let msg_id = cb.get("message").and_then(|m| m.get("message_id")).and_then(|v| v.as_i64());

                    if !self.is_user_allowed(user_id) { continue; }

                    // Answer callback to dismiss loading indicator
                    let _ = self.client
                        .post(&self.api_url("answerCallbackQuery"))
                        .json(&serde_json::json!({ "callback_query_id": cb_id }))
                        .timeout(std::time::Duration::from_secs(5))
                        .send().await;

                    let text = format!("[callback: {}]", data);
                    let _ = tx.send(InboundMessage {
                        service: "telegram".into(), user_id, user_name, text,
                        attachments: vec![], message_id: msg_id,
                        reply_ctx: ReplyContext::Telegram { chat_id },
                    }).await;
                    continue;
                }

                // Handle regular message
                let Some(msg) = update.get("message") else { continue };
                let chat_id = msg.get("chat").and_then(|c| c.get("id")).and_then(|v| v.as_i64()).unwrap_or(0);
                let user_id = msg.get("from").and_then(|f| f.get("id")).and_then(|v| v.as_i64()).unwrap_or(0);
                let user_name = msg.get("from").and_then(|f| f.get("first_name").or_else(|| f.get("username"))).and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                let message_id = msg.get("message_id").and_then(|v| v.as_i64());

                if !self.is_user_allowed(user_id) {
                    log::warn!("[telegram] rejected user {}", user_id); continue;
                }

                // Extract text (from text or caption)
                let text = msg.get("text").or_else(|| msg.get("caption"))
                    .and_then(|v| v.as_str()).unwrap_or("").to_string();

                // Extract media attachments
                let mut attachments = Vec::new();
                if let Some(attachment) = self.extract_attachment(msg, &downloads_dir).await {
                    attachments.push(attachment);
                }

                // Skip if no text and no attachments
                if text.is_empty() && attachments.is_empty() { continue; }

                if tx.send(InboundMessage {
                    service: "telegram".into(), user_id, user_name, text,
                    attachments, message_id,
                    reply_ctx: ReplyContext::Telegram { chat_id },
                }).await.is_err() {
                    log::error!("[telegram] channel send failed — receiver may be dead, but keeping poll alive");
                    if cancellable_sleep(&cancel, std::time::Duration::from_secs(5)).await { return; }
                }
            }
        }
    }
}

async fn cancellable_sleep(cancel: &Notify, dur: std::time::Duration) -> bool {
    tokio::select! {
        biased;
        _ = cancel.notified() => true,
        _ = tokio::time::sleep(dur) => false,
    }
}

/// Evict oldest files from downloads dir — delegates to shared attachments module.
fn evict_downloads(dir: &std::path::Path) {
    crate::attachments::evict(
        dir,
        crate::attachments::EvictionPolicy::MaxBytes(DOWNLOADS_MAX_BYTES),
    );
}

impl MessagingService for TelegramService {
    fn id(&self) -> &str { "telegram" }
    fn start(
        self: Box<Self>,
        tx: tokio::sync::mpsc::Sender<InboundMessage>,
        cancel: Arc<Notify>,
        status: Arc<dyn BridgeStatusSink>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(async move { self.poll_updates(tx, cancel, status).await; })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allowed_users_empty_blocks_all() {
        assert!(!user_allowed(&[], 123));
    }

    #[test]
    fn test_allowed_users_filter() {
        let list = vec!["111".into(), "222".into()];
        assert!(user_allowed(&list, 111));
        assert!(user_allowed(&list, 222));
        assert!(!user_allowed(&list, 333));
    }
}
