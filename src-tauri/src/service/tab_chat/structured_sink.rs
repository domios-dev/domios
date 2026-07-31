//! Tauri sink for structured chat deltas from DB-backed providers.
//! Emits `tab-chat-msg-{id}` events carrying already-parsed `ChatMsg`s;
//! the frontend merges them by id. Mirrored to the SSE bus so the web
//! mirror's chat view streams too.

use std::sync::Arc;

use tauri::AppHandle;

use crate::agent::chat_types::ChatMsg;
use crate::multiplexer::commands::MuxState;

pub trait StructuredSink: Send + Sync {
    fn emit(&self, tab_id: &str, messages: Vec<ChatMsg>, cursor: Option<String>);
}

#[derive(serde::Serialize)]
struct ChatMsgEvent<'a> {
    tab_id: &'a str,
    messages: &'a [ChatMsg],
    cursor: Option<&'a str>,
    source_session_id: Option<&'a str>,
    session_binding_nonce: Option<&'a str>,
}

pub fn emit_chat_msgs(
    app: &AppHandle,
    tab_id: &str,
    messages: &[ChatMsg],
    cursor: Option<&str>,
    source_session_id: Option<&str>,
    session_binding_nonce: Option<&str>,
) {
    let event = format!("tab-chat-msg-{}", tab_id);
    let payload = ChatMsgEvent {
        tab_id,
        messages,
        cursor,
        source_session_id,
        session_binding_nonce,
    };
    crate::api::events::emit_everywhere(
        app,
        &event,
        serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null),
    );
}

pub struct TauriStructuredSink {
    app: AppHandle,
    mux: MuxState,
    source_session_id: Option<String>,
    session_binding_nonce: Option<String>,
    send_ledger: Option<Arc<crate::service::chat_send_transaction::ChatSendLedger>>,
}

impl StructuredSink for TauriStructuredSink {
    fn emit(&self, tab_id: &str, messages: Vec<ChatMsg>, cursor: Option<String>) {
        reconcile_current_session_turns(
            &self.app,
            &self.mux,
            self.send_ledger.as_deref(),
            tab_id,
            self.source_session_id.as_deref(),
            self.session_binding_nonce.as_deref(),
            &messages,
        );
        emit_chat_msgs(
            &self.app,
            tab_id,
            &messages,
            cursor.as_deref(),
            self.source_session_id.as_deref(),
            self.session_binding_nonce.as_deref(),
        );
    }
}

pub(crate) fn make_sink(
    app: AppHandle,
    mux: MuxState,
    source_session_id: Option<String>,
    session_binding_nonce: Option<String>,
    send_ledger: Option<Arc<crate::service::chat_send_transaction::ChatSendLedger>>,
) -> Arc<dyn StructuredSink> {
    Arc::new(TauriStructuredSink {
        app,
        mux,
        source_session_id,
        session_binding_nonce,
        send_ledger,
    })
}

/// Durable wrapper for chatBackend (shell) tabs: forwards every emitted delta
/// to the wrapped UI sink AND UPSERTs it into the session's JSONL so the
/// transcript survives a refresh / VM reload / daemon restart. chatBackend tabs
/// have no provider-side store, so this host-side JSONL IS the durable truth
/// (see [`super::chatbackend_history`]).
pub struct ChatBackendStructuredSink {
    inner: Arc<dyn StructuredSink>,
    history: super::chatbackend_history::SharedHistory,
}

impl ChatBackendStructuredSink {
    /// `meta` is the openSession config persisted as the JSONL `_meta` header so
    /// a restored tab can lazily re-open and continue.
    ///
    /// The history is the SHARED per-session mirror (see
    /// [`super::chatbackend_history::ChatBackendHistory::shared`]) — the same
    /// instance the gateway upserts host inbound rows into, so a poll flush here
    /// merges with (never clobbers) those rows.
    pub fn new(
        inner: Arc<dyn StructuredSink>,
        plugin_id: &str,
        session_id: &str,
        meta: Option<&serde_json::Value>,
    ) -> Self {
        let history = super::chatbackend_history::ChatBackendHistory::shared(plugin_id, session_id);
        if let Some(cfg) = meta {
            history.lock().ensure_meta(cfg);
        }
        Self { inner, history }
    }
}

impl StructuredSink for ChatBackendStructuredSink {
    fn emit(&self, tab_id: &str, messages: Vec<ChatMsg>, cursor: Option<String>) {
        let messages = drop_already_stored_inbound(messages);
        if messages.is_empty() {
            return;
        }
        // Persist first (cheap, deduped), then forward to the UI sink.
        self.history.lock().upsert(&messages);
        self.inner.emit(tab_id, messages, cursor);
    }
}

fn drop_already_stored_inbound(messages: Vec<ChatMsg>) -> Vec<ChatMsg> {
    messages
        .into_iter()
        .filter(|msg| !is_already_stored_inbound(msg))
        .collect()
}

fn is_already_stored_inbound(msg: &ChatMsg) -> bool {
    msg.kind == "user"
        && crate::message_markers::parse(&msg.content)
            .map(|marker| marker.kind == "inbound")
            .unwrap_or(false)
}

/// Adapter that turns a JSONL [`DeltaSink`] (RichMessage) into the structured
/// `tab-chat-msg-{id}` stream: it flattens each RichMessage via
/// `rich_to_chat_msgs` and emits `ChatMsg`s — the SAME shape the snapshot and
/// the DB-backed providers use. This is what lets JSONL providers (claude,
/// codex) and sqlite providers (opencode) share one client rendering path.
pub struct FlattenDeltaSink {
    app: AppHandle,
    mux: MuxState,
    source_session_id: Option<String>,
    session_binding_nonce: Option<String>,
    send_ledger: Option<Arc<crate::service::chat_send_transaction::ChatSendLedger>>,
    /// Monotonic id base for providers whose rows carry no uuid; uuid-bearing
    /// rows ignore it. Seeded past the snapshot's message count so stream ids
    /// never collide with snapshot ids.
    base: std::sync::atomic::AtomicUsize,
}

impl super::tailer::DeltaSink for FlattenDeltaSink {
    fn emit(&self, tab_id: &str, messages: Vec<crate::agent::chat_types::RichMessage>) {
        use std::sync::atomic::Ordering;
        let mut out: Vec<ChatMsg> = Vec::new();
        for r in &messages {
            let base = self.base.fetch_add(1, Ordering::Relaxed);
            out.extend(super::rich_to_chat_msgs(r, base));
        }
        if out.is_empty() {
            return;
        }
        reconcile_current_session_turns(
            &self.app,
            &self.mux,
            self.send_ledger.as_deref(),
            tab_id,
            self.source_session_id.as_deref(),
            self.session_binding_nonce.as_deref(),
            &out,
        );
        let event = format!("tab-chat-msg-{}", tab_id);
        let payload = ChatMsgEvent {
            tab_id,
            messages: &out,
            cursor: None,
            source_session_id: self.source_session_id.as_deref(),
            session_binding_nonce: self.session_binding_nonce.as_deref(),
        };
        crate::api::events::emit_everywhere(
            &self.app,
            &event,
            serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null),
        );
    }
}

pub(crate) fn make_flatten_sink(
    app: AppHandle,
    mux: MuxState,
    base_start: usize,
    source_session_id: Option<String>,
    session_binding_nonce: Option<String>,
    send_ledger: Option<Arc<crate::service::chat_send_transaction::ChatSendLedger>>,
) -> Arc<dyn super::tailer::DeltaSink> {
    Arc::new(FlattenDeltaSink {
        app,
        mux,
        source_session_id,
        session_binding_nonce,
        send_ledger,
        base: std::sync::atomic::AtomicUsize::new(base_start),
    })
}

fn reconcile_current_session_turns(
    app: &AppHandle,
    mux: &MuxState,
    ledger: Option<&crate::service::chat_send_transaction::ChatSendLedger>,
    tab_id: &str,
    source_session_id: Option<&str>,
    source_binding_nonce: Option<&str>,
    messages: &[ChatMsg],
) {
    for status in current_binding_turn_confirmations(
        mux,
        ledger,
        tab_id,
        source_session_id,
        source_binding_nonce,
        messages,
    ) {
        crate::api::events::emit_everywhere(app, "chat-send-status", &status);
    }
}

/// The full live confirmation gate: only turns attributed to the tab's CURRENT
/// bound session (id + nonce) may consume pending sends from the ledger.
pub(crate) fn current_binding_turn_confirmations(
    mux: &MuxState,
    ledger: Option<&crate::service::chat_send_transaction::ChatSendLedger>,
    tab_id: &str,
    source_session_id: Option<&str>,
    source_binding_nonce: Option<&str>,
    messages: &[ChatMsg],
) -> Vec<crate::service::chat_send_transaction::ChatSendStatus> {
    let (Some(ledger), Some(source_session_id)) = (ledger, source_session_id) else {
        return Vec::new();
    };
    if !source_is_current_binding(mux, tab_id, source_session_id, source_binding_nonce) {
        return Vec::new();
    }
    crate::service::chat_send_transaction::delivered_turn_confirmations(
        ledger,
        tab_id,
        source_session_id,
        source_binding_nonce,
        messages,
    )
}

fn source_is_current_binding(
    mux: &MuxState,
    tab_id: &str,
    source_session_id: &str,
    source_binding_nonce: Option<&str>,
) -> bool {
    let current = {
        let mux = mux.read();
        mux.get_tab(tab_id).map(|tab| {
            (
                tab.agent_session_id.clone(),
                tab.session_binding_nonce.clone(),
            )
        })
    };
    current.is_some_and(|(session_id, binding_nonce)| {
        session_id.as_deref() == Some(source_session_id)
            && binding_nonce.as_deref() == source_binding_nonce
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock_hold::TimedRwLock;
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::tab_chat::chatbackend_history;

    #[derive(Default)]
    struct CountingSink {
        emits: std::sync::atomic::AtomicUsize,
    }
    impl StructuredSink for CountingSink {
        fn emit(&self, _tab_id: &str, _messages: Vec<ChatMsg>, _cursor: Option<String>) {
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
    fn stale_session_sink_cannot_supply_delivery_evidence_after_rebind() {
        let (state, tab_id) = MultiplexerState::new();
        let mux = Arc::new(TimedRwLock::new("chat-send-binding-test", state));
        {
            let mut state = mux.write();
            let tab = state.get_tab_mut(&tab_id).unwrap();
            tab.agent_session_id = Some("session-old".into());
            tab.session_binding_nonce = Some("binding-old".into());
        }

        assert!(source_is_current_binding(
            &mux,
            &tab_id,
            "session-old",
            Some("binding-old")
        ));

        {
            let mut state = mux.write();
            let tab = state.get_tab_mut(&tab_id).unwrap();
            tab.agent_session_id = Some("session-new".into());
            tab.session_binding_nonce = Some("binding-new".into());
        }

        assert!(!source_is_current_binding(
            &mux,
            &tab_id,
            "session-old",
            Some("binding-old")
        ));
        assert!(source_is_current_binding(
            &mux,
            &tab_id,
            "session-new",
            Some("binding-new")
        ));
    }

    /// Inverse guard for the missing-store/rebind case: a send whose session
    /// store had not materialized carries no anchor, and still reaches `Sent`
    /// from a genuine attributable bound-session turn arriving through the
    /// live gate — no store-watch, tailer, or other readiness machinery.
    #[test]
    fn missing_store_send_confirms_from_attributable_bound_session_turn() {
        use crate::service::chat_send_transaction::{
            ChatSendLedger, ChatSendState, SendLiveBinding,
        };

        let (state, tab_id) = MultiplexerState::new();
        let mux = Arc::new(TimedRwLock::new("chat-send-live-confirm-test", state));
        {
            let mut state = mux.write();
            let tab = state.get_tab_mut(&tab_id).unwrap();
            tab.agent_session_id = Some("sess-live".into());
            tab.session_binding_nonce = Some("nonce-live".into());
        }

        let ledger = ChatSendLedger::default();
        let send = ledger
            .try_start_scoped(
                &tab_id,
                "deploy the fix",
                None,
                Some(SendLiveBinding {
                    session_id: Some("sess-live".into()),
                    binding_nonce: Some("nonce-live".into()),
                }),
            )
            .unwrap();
        let after_transport = ledger.finish(&send.send_id, Ok(())).unwrap();
        assert_eq!(
            after_transport.state,
            ChatSendState::Pending,
            "bare transport success never proves an unanchored send"
        );

        let user_turn = [ChatMsg {
            id: "turn-1".into(),
            kind: "user".into(),
            content: "deploy the fix".into(),
            ..Default::default()
        }];
        let stale = current_binding_turn_confirmations(
            &mux,
            Some(&ledger),
            &tab_id,
            Some("sess-old"),
            Some("nonce-old"),
            &user_turn,
        );
        assert!(stale.is_empty(), "stale-scope turns cannot confirm");

        let confirmed = current_binding_turn_confirmations(
            &mux,
            Some(&ledger),
            &tab_id,
            Some("sess-live"),
            Some("nonce-live"),
            &user_turn,
        );
        assert_eq!(confirmed.len(), 1);
        assert_eq!(confirmed[0].send_id, send.send_id);
        assert_eq!(confirmed[0].state, ChatSendState::Sent);
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
        sink.emit("tab-1", vec![msg("m1", "He")], None);
        sink.emit("tab-1", vec![msg("m1", "Hello")], None);

        // Forwarded to the UI sink each time.
        assert_eq!(inner.emits.load(std::sync::atomic::Ordering::Relaxed), 2);
        // But the JSONL holds ONE line (UPSERT by id), with the final content.
        let read = chatbackend_history::read_messages(&path);
        assert_eq!(read.len(), 1, "one line per message id");
        assert_eq!(read[0].content, "Hello");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn host_inbound_survives_poll_loop_flush() {
        let plugin = "test-single-writer-unique";
        let session = "sw-sid";
        let path = chatbackend_history::session_path(plugin, session);
        let _ = std::fs::remove_file(&path);

        // 1) Poll loop starts FIRST: the sink seeds its mirror from the (empty)
        //    JSONL and keeps it for the loop's lifetime.
        let inner = Arc::new(CountingSink::default());
        let sink = ChatBackendStructuredSink::new(inner, plugin, session, None);

        // 2) Gateway writes the host inbound (clean body, structured sender)
        //    AFTER the sink already exists.
        chatbackend_history::ChatBackendHistory::shared(plugin, session)
            .lock()
            .upsert(&[ChatMsg {
                id: "inbound-1".into(),
                kind: "user".into(),
                content: "ping from Gauss".into(),
                from_label: Some("Gauss".into()),
                inbound_kind: Some("tab".into()),
                ..Default::default()
            }]);

        //    inbound row (the loop never saw it).
        sink.emit("tab-1", vec![msg("reply-1", "pong")], None);

        // 3) The JSONL retains BOTH rows, in order, inbound body still clean.
        let read = chatbackend_history::read_messages(&path);
        let ids: Vec<&str> = read.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["inbound-1", "reply-1"], "host inbound not clobbered");
        assert_eq!(read[0].content, "ping from Gauss");
        assert!(!read[0].content.contains("-=-codeterm"));
        assert_eq!(read[1].content, "pong");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn chatbackend_sink_drops_plugin_echo_of_host_stored_inbound() {
        let plugin = "test-sink-inbound-filter";
        let session = "sink-sid";
        let path = chatbackend_history::session_path(plugin, session);
        let _ = std::fs::remove_file(&path);

        let inner = Arc::new(CountingSink::default());
        let sink = ChatBackendStructuredSink::new(inner.clone(), plugin, session, None);
        let inbound = crate::message_markers::Marker::new("inbound")
            .attr("kind", "tab")
            .attr("from_label", "Alice")
            .attr("from_tab", "tab-a")
            .payload("hello")
            .build();

        sink.emit(
            "tab-1",
            vec![
                ChatMsg {
                    id: "echo".into(),
                    kind: "user".into(),
                    content: inbound,
                    ..Default::default()
                },
                msg("reply", "ack"),
            ],
            None,
        );

        assert_eq!(inner.emits.load(std::sync::atomic::Ordering::Relaxed), 1);
        let read = chatbackend_history::read_messages(&path);
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].id, "reply");

        let _ = std::fs::remove_file(&path);
    }
}
