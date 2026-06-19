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

pub fn emit_chat_msgs(app: &AppHandle, pane_id: &str, messages: &[ChatMsg], cursor: Option<&str>) {
    let event = format!("pane-chat-msg-{}", pane_id);
    let payload = ChatMsgEvent {
        pane_id,
        messages,
        cursor,
    };
    crate::api::events::emit_everywhere(
        app,
        &event,
        serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null),
    );
}

pub struct TauriStructuredSink {
    app: AppHandle,
}

impl StructuredSink for TauriStructuredSink {
    fn emit(&self, pane_id: &str, messages: Vec<ChatMsg>, cursor: Option<String>) {
        emit_chat_msgs(&self.app, pane_id, &messages, cursor.as_deref());
    }
}

pub fn make_sink(app: AppHandle) -> Arc<dyn StructuredSink> {
    Arc::new(TauriStructuredSink { app })
}

/// Durable wrapper for chatBackend (shell) panes: forwards every emitted delta
/// to the wrapped UI sink AND UPSERTs it into the session's JSONL so the
/// transcript survives a refresh / VM reload / daemon restart. chatBackend panes
/// have no provider-side store, so this host-side JSONL IS the durable truth
/// (see [`super::chatbackend_history`]).
pub struct ChatBackendStructuredSink {
    inner: Arc<dyn StructuredSink>,
    history: parking_lot::Mutex<super::chatbackend_history::ChatBackendHistory>,
}

impl ChatBackendStructuredSink {
    /// `meta` is the openSession config persisted as the JSONL `_meta` header so
    /// a restored pane can lazily re-open and continue.
    pub fn new(
        inner: Arc<dyn StructuredSink>,
        plugin_id: &str,
        session_id: &str,
        meta: Option<&serde_json::Value>,
    ) -> Self {
        let mut history =
            super::chatbackend_history::ChatBackendHistory::open(plugin_id, session_id);
        if let Some(cfg) = meta {
            history.ensure_meta(cfg);
        }
        Self {
            inner,
            history: parking_lot::Mutex::new(history),
        }
    }
}

impl StructuredSink for ChatBackendStructuredSink {
    fn emit(&self, pane_id: &str, messages: Vec<ChatMsg>, cursor: Option<String>) {
        // Persist first (cheap, deduped), then forward to the UI sink.
        self.history.lock().upsert(&messages);
        self.inner.emit(pane_id, messages, cursor);
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::pane_chat::chatbackend_history;

    #[derive(Default)]
    struct CountingSink {
        emits: std::sync::atomic::AtomicUsize,
    }
    impl StructuredSink for CountingSink {
        fn emit(&self, _pane_id: &str, _messages: Vec<ChatMsg>, _cursor: Option<String>) {
            self.emits
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    fn msg(id: &str, content: &str) -> ChatMsg {
        ChatMsg {
            id: id.into(),
            kind: "assistant".into(),
            content: content.into(),
            ..Default::default()
        }
    }

    #[test]
    fn chatbackend_sink_persists_and_forwards() {
        let plugin = "test-sink-plugin-unique";
        let session = "sink-sid";
        let path = chatbackend_history::session_path(plugin, session);
        let _ = std::fs::remove_file(&path);

        let inner = Arc::new(CountingSink::default());
        let sink = ChatBackendStructuredSink::new(inner.clone(), plugin, session, None);

        // A streaming message grows in place across two emits.
        sink.emit("pane-1", vec![msg("m1", "He")], None);
        sink.emit("pane-1", vec![msg("m1", "Hello")], None);

        // Forwarded to the UI sink each time.
        assert_eq!(inner.emits.load(std::sync::atomic::Ordering::Relaxed), 2);
        // But the JSONL holds ONE line (UPSERT by id), with the final content.
        let read = chatbackend_history::read_messages(&path);
        assert_eq!(read.len(), 1, "one line per message id");
        assert_eq!(read[0].content, "Hello");

        let _ = std::fs::remove_file(&path);
    }
}
