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

/// Adapter that turns a JSONL [`DeltaSink`] (RichMessage) into the structured
/// `pane-chat-msg-{id}` stream: it flattens each RichMessage via
/// `rich_to_chat_msgs` and emits `ChatMsg`s — the SAME shape the snapshot and
/// the DB-backed providers use. This is what lets JSONL providers (claude,
/// codex) and sqlite providers (opencode) share one client rendering path.
pub struct FlattenDeltaSink {
    app: AppHandle,
    /// Monotonic id base for providers whose rows carry no uuid; uuid-bearing
    /// rows ignore it. Seeded past the snapshot's message count so stream ids
    /// never collide with snapshot ids.
    base: std::sync::atomic::AtomicUsize,
}

impl super::tailer::DeltaSink for FlattenDeltaSink {
    fn emit(&self, pane_id: &str, messages: Vec<crate::agent::chat_types::RichMessage>) {
        use std::sync::atomic::Ordering;
        let mut out: Vec<ChatMsg> = Vec::new();
        for r in &messages {
            let base = self.base.fetch_add(1, Ordering::Relaxed);
            out.extend(super::rich_to_chat_msgs(r, base));
        }
        if out.is_empty() {
            return;
        }
        let event = format!("pane-chat-msg-{}", pane_id);
        let payload = ChatMsgEvent {
            pane_id,
            messages: &out,
            cursor: None,
        };
        crate::api::events::emit_everywhere(
            &self.app,
            &event,
            serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null),
        );
    }
}

pub fn make_flatten_sink(app: AppHandle, base_start: usize) -> Arc<dyn super::tailer::DeltaSink> {
    Arc::new(FlattenDeltaSink {
        app,
        base: std::sync::atomic::AtomicUsize::new(base_start),
    })
}
