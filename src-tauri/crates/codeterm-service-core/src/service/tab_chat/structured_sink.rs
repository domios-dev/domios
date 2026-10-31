//! Tauri sink for structured chat deltas from DB-backed providers and event streams.
//! Emits `tab-chat-msg-{id}` events carrying already-parsed `ChatMsg`s;
//! the frontend merges them by id. Mirrored to the SSE bus so the web
//! mirror's chat view streams too.

use crate::ports::host::UiEvents;
use crate::service::ServiceContext;
use std::sync::Arc;

use crate::agent::chat_types::ChatMsg;
use crate::multiplexer::commands::MuxState;
use crate::service::chat_send_transaction::{LiveProvenance, TurnPosition};
use crate::session::lifecycle::TabLifecycleJournal;

mod sequence;
pub(super) use sequence::clear_tab;
pub use sequence::seed_snapshot;

pub trait StructuredSink: Send + Sync {
    fn observe(&self, _tab: &str, _records: &[crate::agent::chat_types::RichMessage]) {}
    fn emit(&self, tab_id: &str, messages: Vec<ChatMsg>, cursor: Option<String>);
}

#[derive(serde::Serialize)]
struct ChatMsgEvent<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostic_epoch: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostic_live: Option<&'a LiveRuntimeOrigin>,
    tab_id: &'a str,
    messages: &'a [ChatMsg],
    cursor: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_offset: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    offset: Option<u64>,
    source_session_id: Option<&'a str>,
    session_binding_nonce: Option<&'a str>,
}

#[derive(serde::Serialize)]
pub(crate) struct LiveRuntimeOrigin {
    kind: &'static str,
    turn: String,
}

pub(crate) fn live_runtime_origin(
    live: bool,
    before: &codeterm_plugin::agent_runtime::RuntimeContext,
    current: &codeterm_plugin::agent_runtime::RuntimeContext,
    thread: &str,
    item: &ChatMsg,
) -> Option<LiveRuntimeOrigin> {
    let turn = current.active_turn_id.as_deref()?;
    (live
        && !current.tab_id.is_empty()
        && !current.binding_nonce.is_empty()
        && current.epoch > 0
        && !thread.is_empty()
        && before.tab_id == current.tab_id
        && before.epoch == current.epoch
        && before.binding_nonce == current.binding_nonce
        && before.thread_id == current.thread_id
        && current.thread_id.as_deref() == Some(thread)
        && !turn.is_empty()
        && turn.len() <= 128
        && !turn.bytes().any(|byte| byte.is_ascii_control())
        && item.tool_flow_id.as_deref() == Some(turn)
        && item.inherited_from_session_id.is_none())
    .then(|| LiveRuntimeOrigin {
        kind: "runtime_live",
        turn: turn.to_owned(),
    })
}

pub fn emit_chat_msgs(
    ui: &dyn UiEvents,
    tab_id: &str,
    messages: &[ChatMsg],
    cursor: Option<&str>,
    source_session_id: Option<&str>,
    session_binding_nonce: Option<&str>,
) {
    emit_chat_msgs_with_origin(
        ui,
        tab_id,
        messages,
        cursor,
        source_session_id,
        session_binding_nonce,
        None,
    );
}

pub(crate) fn emit_live_runtime_chat_msg(
    ui: &dyn UiEvents,
    tab_id: &str,
    message: &ChatMsg,
    source_session_id: Option<&str>,
    session_binding_nonce: Option<&str>,
    origin: Option<&LiveRuntimeOrigin>,
) {
    emit_chat_msgs_with_origin(
        ui,
        tab_id,
        std::slice::from_ref(message),
        None,
        source_session_id,
        session_binding_nonce,
        origin,
    );
}

fn emit_chat_msgs_with_origin(
    ui: &dyn UiEvents,
    tab_id: &str,
    messages: &[ChatMsg],
    cursor: Option<&str>,
    source_session_id: Option<&str>,
    session_binding_nonce: Option<&str>,
    origin: Option<&LiveRuntimeOrigin>,
) {
    let mut messages: Vec<_> = messages
        .iter()
        .cloned()
        .map(ChatMsg::with_segments)
        .collect();
    sequence::assign(
        tab_id,
        source_session_id,
        session_binding_nonce,
        &mut messages,
    );
    let event = format!("tab-chat-msg-{}", tab_id);
    let trace = crate::service::agent_runtime::model::responsiveness::binding(
        tab_id,
        source_session_id,
        session_binding_nonce,
    );
    let payload = ChatMsgEvent {
        diagnostic_epoch: trace.and_then(|trace| trace.epoch()),
        diagnostic_live: trace.and(origin),
        tab_id,
        messages: &messages,
        cursor,
        start_offset: None,
        offset: None,
        source_session_id,
        session_binding_nonce,
    };
    if let Some(trace) = trace {
        for row in &messages {
            trace.row("host_emit_begin", row);
        }
    }
    ui.emit(
        &event,
        serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null),
    );
    if let Some(trace) = trace {
        for row in &messages {
            trace.row("host_emit_end", row);
        }
    }
}

pub struct TauriStructuredSink {
    journal: Arc<TabLifecycleJournal>,
    ui: Arc<dyn UiEvents>,
    service: std::sync::Weak<ServiceContext>,
    mux: MuxState,
    source_session_id: Option<String>,
    session_binding_nonce: Option<String>,
    send_ledger: Option<Arc<crate::service::chat_send_transaction::ChatSendLedger>>,
}

impl StructuredSink for TauriStructuredSink {
    fn observe(&self, tab: &str, records: &[crate::agent::chat_types::RichMessage]) {
        if let (Some(session), Some(nonce), Some(svc)) = (
            self.source_session_id.as_deref(),
            self.session_binding_nonce.as_deref(),
            self.service.upgrade(),
        ) {
            crate::service::provider_signals::apply_structured(&svc, tab, session, nonce, records);
        }
    }

    fn emit(&self, tab_id: &str, messages: Vec<ChatMsg>, cursor: Option<String>) {
        let messages: Vec<_> = messages.into_iter().map(ChatMsg::with_segments).collect();
        if let (Some(session_id), Some(binding_nonce)) = (
            self.source_session_id.as_deref(),
            self.session_binding_nonce.as_deref(),
        ) {
            crate::service::public_send_readiness::note_bootstrap_turn(
                &self.mux,
                tab_id,
                session_id,
                binding_nonce,
                &messages,
            );
        }
        reconcile_current_session_turns(
            &self.journal,
            self.ui.as_ref(),
            &self.mux,
            self.send_ledger.as_deref(),
            tab_id,
            self.source_session_id.as_deref(),
            self.session_binding_nonce.as_deref(),
            &messages,
            None,
        );
        let pty_id = self
            .mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.pty_id.clone());
        if let Some(pty_id) = pty_id {
            crate::service::pty_io::submit_transaction::wake_submit_cycle(&pty_id);
        }
        emit_chat_msgs(
            self.ui.as_ref(),
            tab_id,
            &messages,
            cursor.as_deref(),
            self.source_session_id.as_deref(),
            self.session_binding_nonce.as_deref(),
        );
        if let Some(service) = self.service.upgrade() {
            service.run_watcher.check_pending_cron(&service, tab_id);
        }
    }
}

pub fn make_sink(
    journal: Arc<TabLifecycleJournal>,
    svc: &ServiceContext,
    mux: MuxState,
    source_session_id: Option<String>,
    session_binding_nonce: Option<String>,
    send_ledger: Option<Arc<crate::service::chat_send_transaction::ChatSendLedger>>,
) -> Arc<dyn StructuredSink> {
    Arc::new(TauriStructuredSink {
        journal,
        ui: svc.ui.clone(),
        service: svc.weak_self(),
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
        && crate::message_markers::parse_host_injected(&msg.content).is_some_and(|marker| {
            matches!(
                marker.kind.as_str(),
                "tab"
                    | "report"
                    | "system"
                    | "reminder"
                    | "mesh"
                    | "telegram"
                    | "pebble"
                    | "plugin"
            )
        })
}

/// Adapter that turns a JSONL [`DeltaSink`] (RichMessage) into the structured
/// `tab-chat-msg-{id}` stream: it flattens each RichMessage via
/// `rich_to_chat_msgs_live` and emits `ChatMsg`s — the SAME shape the snapshot and
/// the DB-backed providers use. This is what lets JSONL providers (claude,
/// codex) and sqlite providers (opencode) share one client rendering path.
pub struct FlattenDeltaSink {
    journal: Arc<TabLifecycleJournal>,
    ui: Arc<dyn UiEvents>,
    service: std::sync::Weak<ServiceContext>,
    mux: MuxState,
    source_session_id: Option<String>,
    session_binding_nonce: Option<String>,
    send_ledger: Option<Arc<crate::service::chat_send_transaction::ChatSendLedger>>,
    /// Monotonic stream `seq`, seeded past the snapshot's message count. Ids come
    /// from each row's uuid or record anchor; only an unanchored row uses it.
    base: std::sync::atomic::AtomicUsize,
}

impl super::tailer::DeltaSink for FlattenDeltaSink {
    fn emit(
        &self,
        tab_id: &str,
        messages: Vec<crate::agent::chat_types::RichMessage>,
        start_offset: u64,
        offset: u64,
    ) {
        self.emit_inner(tab_id, messages, start_offset, offset, None);
    }

    fn emit_sourced(
        &self,
        tab_id: &str,
        messages: Vec<crate::agent::chat_types::RichMessage>,
        range: super::tailer::SourceRange,
    ) {
        self.emit_inner(
            tab_id,
            messages,
            range.start_offset,
            range.offset,
            Some(range),
        );
    }
}

impl FlattenDeltaSink {
    fn emit_inner(
        &self,
        tab_id: &str,
        messages: Vec<crate::agent::chat_types::RichMessage>,
        start_offset: u64,
        offset: u64,
        source_range: Option<super::tailer::SourceRange>,
    ) {
        use std::sync::atomic::Ordering;
        if let (Some(session), Some(nonce), Some(svc)) = (
            self.source_session_id.as_deref(),
            self.session_binding_nonce.as_deref(),
            self.service.upgrade(),
        ) {
            crate::service::provider_signals::apply(&svc, tab_id, session, nonce, &messages);
            crate::service::background_work::apply(&svc, tab_id, session, nonce, &messages);
            if let Some(context) = messages
                .iter()
                .rev()
                .find_map(|message| message.context_usage.clone())
            {
                crate::service::context_feed::apply(&svc, tab_id, session, nonce, context);
            }
        }
        let mut out: Vec<ChatMsg> = Vec::new();
        let mut positions = Vec::new();
        for r in &messages {
            let base = self.base.fetch_add(1, Ordering::Relaxed);
            let flattened = super::rich_to_chat_msgs_live(r, base);
            positions.extend(
                std::iter::repeat(TurnPosition {
                    record_start: r.record_start,
                    record_row: r.record_row,
                })
                .take(flattened.len()),
            );
            out.extend(flattened);
        }
        if out.is_empty() && start_offset == offset {
            return;
        }
        if let (Some(session_id), Some(binding_nonce)) = (
            self.source_session_id.as_deref(),
            self.session_binding_nonce.as_deref(),
        ) {
            crate::service::public_send_readiness::note_bootstrap_turn(
                &self.mux,
                tab_id,
                session_id,
                binding_nonce,
                &out,
            );
        }
        let cursor = source_range.as_ref().and_then(|range| {
            let incarnation = range.incarnation.as_ref()?;
            super::resume::encode_cursor(
                super::resume_cursor::ChatEpoch {
                    source_session_id: self.source_session_id.clone()?,
                    session_binding_nonce: self.session_binding_nonce.clone(),
                },
                incarnation,
                offset,
            )
        });
        let provenance = source_range.map(|range| LiveProvenance {
            source_session_id: self.source_session_id.clone().unwrap_or_default(),
            source_binding_nonce: self.session_binding_nonce.clone(),
            range,
            positions,
        });
        reconcile_current_session_turns(
            &self.journal,
            self.ui.as_ref(),
            &self.mux,
            self.send_ledger.as_deref(),
            tab_id,
            self.source_session_id.as_deref(),
            self.session_binding_nonce.as_deref(),
            &out,
            provenance.as_ref(),
        );
        let event = format!("tab-chat-msg-{}", tab_id);
        let payload = ChatMsgEvent {
            diagnostic_epoch: None,
            diagnostic_live: None,
            tab_id,
            messages: &out,
            cursor: cursor.as_deref(),
            start_offset: (start_offset != offset).then_some(start_offset),
            offset: (start_offset != offset).then_some(offset),
            source_session_id: self.source_session_id.as_deref(),
            session_binding_nonce: self.session_binding_nonce.as_deref(),
        };
        self.ui.emit(
            &event,
            serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null),
        );
    }
}

pub fn make_flatten_sink(
    journal: Arc<TabLifecycleJournal>,
    svc: &ServiceContext,
    mux: MuxState,
    base_start: usize,
    source_session_id: Option<String>,
    session_binding_nonce: Option<String>,
    send_ledger: Option<Arc<crate::service::chat_send_transaction::ChatSendLedger>>,
) -> Arc<dyn super::tailer::DeltaSink> {
    Arc::new(FlattenDeltaSink {
        journal,
        ui: svc.ui.clone(),
        service: svc.weak_self(),
        mux,
        source_session_id,
        session_binding_nonce,
        send_ledger,
        base: std::sync::atomic::AtomicUsize::new(base_start),
    })
}

fn reconcile_current_session_turns(
    journal: &TabLifecycleJournal,
    ui: &dyn UiEvents,
    mux: &MuxState,
    ledger: Option<&crate::service::chat_send_transaction::ChatSendLedger>,
    tab_id: &str,
    source_session_id: Option<&str>,
    source_binding_nonce: Option<&str>,
    messages: &[ChatMsg],
    provenance: Option<&LiveProvenance>,
) {
    note_current_session_conversation(
        journal,
        mux,
        ledger,
        tab_id,
        source_session_id,
        source_binding_nonce,
        messages,
    );
    for status in current_binding_turn_confirmations(
        mux,
        ledger,
        tab_id,
        source_session_id,
        source_binding_nonce,
        messages,
        provenance,
    ) {
        ui.emit(
            "chat-send-status",
            serde_json::to_value(status).unwrap_or_default(),
        );
    }
}

fn note_current_session_conversation(
    journal: &TabLifecycleJournal,
    mux: &MuxState,
    ledger: Option<&crate::service::chat_send_transaction::ChatSendLedger>,
    tab_id: &str,
    source_session_id: Option<&str>,
    source_binding_nonce: Option<&str>,
    messages: &[ChatMsg],
) {
    let Some(session_id) = source_session_id.filter(|_| ledger.is_some()) else {
        return;
    };
    if !source_is_current_binding(mux, tab_id, session_id, source_binding_nonce)
        || !messages.iter().any(|message| message.kind == "user")
    {
        return;
    }
    let Some(key) = mux.read().lifecycle_key(tab_id) else {
        return;
    };
    if let Err(error) = journal.note_conversation_started(key, session_id) {
        log::warn!("[conversation-evidence] Tab {tab_id}: {error}");
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
    provenance: Option<&LiveProvenance>,
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
        provenance,
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
mod offset_tests;

#[cfg(test)]
mod tests;
