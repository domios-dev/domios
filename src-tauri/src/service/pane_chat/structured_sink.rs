//! Tauri sink for structured chat deltas from DB-backed providers.
//! Emits `pane-chat-msg-{id}` events carrying already-parsed `ChatMsg`s;
//! the frontend merges them by id. Mirrored to the SSE bus so the web
//! mirror's chat view streams too.

use std::sync::Arc;

use tauri::AppHandle;

use crate::agent::chat_types::ChatMsg;

pub trait StructuredSink: Send + Sync {
    fn emit(&self, pane_id: &str, messages: Vec<ChatMsg>, cursor: Option<String>);
}

#[derive(serde::Serialize)]
struct ChatMsgEvent<'a> {
    pane_id: &'a str,
    messages: &'a [ChatMsg],
    cursor: Option<&'a str>,
}

pub struct TauriStructuredSink {
    app: AppHandle,
}

impl StructuredSink for TauriStructuredSink {
    fn emit(&self, pane_id: &str, messages: Vec<ChatMsg>, cursor: Option<String>) {
        let event = format!("pane-chat-msg-{}", pane_id);
        let payload = ChatMsgEvent {
            pane_id,
            messages: &messages,
            cursor: cursor.as_deref(),
        };
        crate::api::events::emit_everywhere(
            &self.app,
            &event,
            serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null),
        );
    }
}

pub fn make_sink(app: AppHandle) -> Arc<dyn StructuredSink> {
    Arc::new(TauriStructuredSink { app })
}
