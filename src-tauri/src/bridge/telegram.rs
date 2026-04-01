//! Telegram Bot API via reqwest — no extra crates.

use std::pin::Pin;
use std::future::Future;
use super::traits::{InboundMessage, MessagingService, ReplyContext};

pub const TELEGRAM_MAX_LEN: usize = 4096;

fn token_path() -> std::path::PathBuf {
    crate::service::general_agent::secrets_dir().join("telegram.token")
}

pub fn read_token() -> Option<String> {
    std::fs::read_to_string(token_path()).ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub struct TelegramService {
    token: String,
    allowed_users: Vec<String>,
    client: reqwest::Client,
}

impl TelegramService {
    pub fn new(token: String, allowed_users: Vec<String>) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(40)) // > Telegram's 30s long poll
            .tcp_keepalive(std::time::Duration::from_secs(15))
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { token, allowed_users, client }
    }

    fn api_url(&self, method: &str) -> String {
        format!("https://api.telegram.org/bot{}/{}", self.token, method)
    }

    fn is_user_allowed(&self, user_id: i64) -> bool {
        self.allowed_users.is_empty() || self.allowed_users.contains(&user_id.to_string())
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

    async fn poll_updates(self: Box<Self>, tx: tokio::sync::mpsc::Sender<InboundMessage>) {
        let mut offset: i64 = 0;
        let mut error_count: u32 = 0;
        let mut poll_count: u64 = 0;

        loop {
            poll_count += 1;
            if poll_count % 10 == 1 {
                log::info!("[telegram] poll #{}, offset={}, errors={}", poll_count, offset, error_count);
            }

            let resp = self.client
                .post(&self.api_url("getUpdates"))
                .json(&serde_json::json!({ "offset": offset, "timeout": 30, "allowed_updates": ["message"] }))
                .send().await; // client-level timeout handles this (40s)

            let body = match resp {
                Ok(r) if r.status().is_success() => {
                    error_count = 0;
                    match r.json::<serde_json::Value>().await {
                        Ok(v) => v,
                        Err(e) => { log::warn!("[telegram] parse error: {}", e); tokio::time::sleep(std::time::Duration::from_secs(1)).await; continue; }
                    }
                }
                Ok(r) if r.status().as_u16() == 429 => {
                    let wait = 30u64; // Telegram rate limit
                    log::warn!("[telegram] rate limited, waiting {}s", wait);
                    tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                    continue;
                }
                Ok(r) => {
                    error_count += 1;
                    log::warn!("[telegram] API error {}", r.status());
                    let backoff = (2u64.pow(error_count.min(6))).min(60);
                    tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
                    continue;
                }
                Err(e) => {
                    error_count += 1;
                    log::warn!("[telegram] poll error (count={}): {}", error_count, e);
                    let backoff = (2u64.pow(error_count.min(6))).min(60);
                    tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
                    continue;
                }
            };

            let Some(updates) = body.get("result").and_then(|v| v.as_array()) else {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await; continue;
            };

            for update in updates {
                if let Some(uid) = update.get("update_id").and_then(|v| v.as_i64()) { offset = uid + 1; }
                let Some(msg) = update.get("message") else { continue };
                let text = msg.get("text").and_then(|v| v.as_str()).unwrap_or("");
                if text.is_empty() { continue; }
                let chat_id = msg.get("chat").and_then(|c| c.get("id")).and_then(|v| v.as_i64()).unwrap_or(0);
                let user_id = msg.get("from").and_then(|f| f.get("id")).and_then(|v| v.as_i64()).unwrap_or(0);
                let user_name = msg.get("from").and_then(|f| f.get("first_name").or_else(|| f.get("username"))).and_then(|v| v.as_str()).unwrap_or("unknown").to_string();

                if !self.is_user_allowed(user_id) {
                    log::warn!("[telegram] rejected user {}", user_id); continue;
                }

                if tx.send(InboundMessage { service: "telegram".into(), user_id, user_name, text: text.into(), reply_ctx: ReplyContext::Telegram { chat_id } }).await.is_err() {
                    log::error!("[telegram] channel send failed — receiver may be dead, but keeping poll alive");
                    // Don't return — keep polling. Messages will be dropped but polling stays alive
                    // so we can recover if receiver comes back.
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }
    }
}

impl MessagingService for TelegramService {
    fn id(&self) -> &str { "telegram" }
    fn start(self: Box<Self>, tx: tokio::sync::mpsc::Sender<InboundMessage>) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(async move { self.poll_updates(tx).await; })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allowed_users_empty() {
        let svc = TelegramService::new("t".into(), vec![]);
        assert!(svc.is_user_allowed(123));
    }

    #[test]
    fn test_allowed_users_filter() {
        let svc = TelegramService::new("t".into(), vec!["111".into(), "222".into()]);
        assert!(svc.is_user_allowed(111));
        assert!(!svc.is_user_allowed(333));
    }
}
