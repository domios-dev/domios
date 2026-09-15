//! The forum-topic Bot API surface threads need, behind a trait so fixtures
//! observe every call without a network.

use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::bridge::traits::InlineKeyboard;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TopicFailure {
    /// Topics are off for this bot or the chat is not a forum.
    TopicsUnavailable,
    /// The thread no longer exists.
    ThreadGone,
    Rejected(String),
}

pub fn classify_failure(description: &str) -> TopicFailure {
    let d = description.to_ascii_lowercase();
    let topics_off = d.contains("not a forum")
        || d.contains("forum_disabled")
        || d.contains("only for supergroups")
        || (d.contains("topic") && (d.contains("disabled") || d.contains("not enabled")));
    if topics_off {
        return TopicFailure::TopicsUnavailable;
    }
    if d.contains("thread not found") || d.contains("topic_deleted") || d.contains("topic_id_invalid") {
        return TopicFailure::ThreadGone;
    }
    TopicFailure::Rejected(description.to_string())
}

/// A one-tap link into a topic. Supergroups use the `t.me/c` form; a bot's
/// private chat uses the bot username form.
pub fn topic_link(bot_username: Option<&str>, chat_id: i64, thread_id: i64) -> Option<String> {
    if let Some(internal) = chat_id.to_string().strip_prefix("-100") {
        return Some(format!("https://t.me/c/{internal}/{thread_id}"));
    }
    let username = bot_username.map(str::trim).filter(|u| !u.is_empty())?;
    Some(format!("https://t.me/{username}/{thread_id}"))
}

/// The bot's public user id, which prefixes its token. Never the secret half.
pub fn bot_id_from_token(token: &str) -> i64 {
    token
        .split(':')
        .next()
        .and_then(|id| id.trim().parse().ok())
        .unwrap_or_default()
}

#[async_trait]
pub trait TopicTransport: Send + Sync {
    async fn bot_username(&self) -> Option<String>;
    async fn create_topic(&self, chat_id: i64, name: &str) -> Result<i64, TopicFailure>;
    async fn delete_topic(&self, chat_id: i64, thread_id: i64) -> Result<(), TopicFailure>;
    async fn rename_topic(&self, chat_id: i64, thread_id: i64, name: &str) -> Result<(), TopicFailure>;
    async fn send_html(
        &self,
        chat_id: i64,
        thread_id: Option<i64>,
        html: &str,
        buttons: Option<&InlineKeyboard>,
    ) -> Result<i64, TopicFailure>;
    async fn edit_html(&self, chat_id: i64, message_id: i64, html: &str) -> bool;
}

pub struct TelegramTopicTransport {
    client: reqwest::Client,
    api_base: String,
    token: String,
    username: tokio::sync::OnceCell<String>,
}

impl TelegramTopicTransport {
    pub fn new(token: String) -> Self {
        Self::with_base(token, crate::bridge::telegram::telegram_api_base())
    }

    pub fn with_base(token: String, api_base: String) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            client,
            api_base,
            token,
            username: tokio::sync::OnceCell::new(),
        }
    }

    async fn call(&self, method: &str, payload: Value) -> Result<Value, String> {
        let url = format!(
            "{}/bot{}/{}",
            self.api_base.trim_end_matches('/'),
            self.token,
            method
        );
        let response = self
            .client
            .post(url)
            .json(&payload)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|error| format!("{method}: {}", error.without_url()))?;
        let body: Value = response
            .json()
            .await
            .map_err(|error| format!("{method}: {}", error.without_url()))?;
        if body.get("ok").and_then(Value::as_bool) == Some(true) {
            return Ok(body.get("result").cloned().unwrap_or(Value::Null));
        }
        Err(body
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("unknown Telegram error")
            .to_string())
    }

    async fn send_payload(&self, payload: Value) -> Result<i64, String> {
        let result = self.call("sendMessage", payload).await?;
        result
            .get("message_id")
            .and_then(Value::as_i64)
            .ok_or_else(|| "sendMessage returned no message_id".to_string())
    }
}

#[async_trait]
impl TopicTransport for TelegramTopicTransport {
    async fn bot_username(&self) -> Option<String> {
        self.username
            .get_or_try_init(|| async {
                self.call("getMe", json!({}))
                    .await
                    .ok()
                    .and_then(|me| me.get("username").and_then(Value::as_str).map(str::to_string))
                    .ok_or(())
            })
            .await
            .ok()
            .cloned()
    }

    async fn create_topic(&self, chat_id: i64, name: &str) -> Result<i64, TopicFailure> {
        let result = self
            .call("createForumTopic", json!({ "chat_id": chat_id, "name": name }))
            .await
            .map_err(|description| classify_failure(&description))?;
        result
            .get("message_thread_id")
            .and_then(Value::as_i64)
            .ok_or_else(|| TopicFailure::Rejected("createForumTopic returned no thread".into()))
    }

    async fn delete_topic(&self, chat_id: i64, thread_id: i64) -> Result<(), TopicFailure> {
        self.call(
            "deleteForumTopic",
            json!({ "chat_id": chat_id, "message_thread_id": thread_id }),
        )
        .await
        .map(|_| ())
        .map_err(|description| classify_failure(&description))
    }

    async fn rename_topic(&self, chat_id: i64, thread_id: i64, name: &str) -> Result<(), TopicFailure> {
        self.call(
            "editForumTopic",
            json!({ "chat_id": chat_id, "message_thread_id": thread_id, "name": name }),
        )
        .await
        .map(|_| ())
        .map_err(|description| classify_failure(&description))
    }

    async fn send_html(
        &self,
        chat_id: i64,
        thread_id: Option<i64>,
        html: &str,
        buttons: Option<&InlineKeyboard>,
    ) -> Result<i64, TopicFailure> {
        let mut payload = json!({
            "chat_id": chat_id,
            "text": html,
            "parse_mode": "HTML",
            "disable_web_page_preview": true,
        });
        if let Some(thread_id) = thread_id {
            payload["message_thread_id"] = json!(thread_id);
        }
        if let Some(keyboard) = buttons {
            payload["reply_markup"] = json!({ "inline_keyboard": keyboard });
        }
        match self.send_payload(payload.clone()).await {
            Ok(message_id) => Ok(message_id),
            Err(description) if description.to_ascii_lowercase().contains("parse") => {
                if let Some(fields) = payload.as_object_mut() {
                    fields.remove("parse_mode");
                }
                self.send_payload(payload)
                    .await
                    .map_err(|description| classify_failure(&description))
            }
            Err(description) => Err(classify_failure(&description)),
        }
    }

    async fn edit_html(&self, chat_id: i64, message_id: i64, html: &str) -> bool {
        let payload = json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "text": html,
            "parse_mode": "HTML",
            "disable_web_page_preview": true,
        });
        match self.call("editMessageText", payload).await {
            Ok(_) => true,
            Err(description) => description.contains("message is not modified"),
        }
    }
}
