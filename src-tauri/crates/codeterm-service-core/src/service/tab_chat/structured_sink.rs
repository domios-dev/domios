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

pub trait StructuredSink: Send + Sync {
    fn observe(&self, _tab: &str, _records: &[crate::agent::chat_types::RichMessage]) {}
    fn emit(&self, tab_id: &str, messages: Vec<ChatMsg>, cursor: Option<String>);
}

#[derive(serde::Serialize)]
struct ChatMsgEvent<'a> {
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

pub fn emit_chat_msgs(
    ui: &dyn UiEvents,
    tab_id: &str,
    messages: &[ChatMsg],
    cursor: Option<&str>,
    source_session_id: Option<&str>,
    session_binding_nonce: Option<&str>,
) {
    let messages: Vec<_> = messages
        .iter()
        .cloned()
        .map(ChatMsg::with_segments)
        .collect();
    let event = format!("tab-chat-msg-{}", tab_id);
    let payload = ChatMsgEvent {
        tab_id,
        messages: &messages,
        cursor,
        start_offset: None,
        offset: None,
        source_session_id,
        session_binding_nonce,
    };
    ui.emit(
        &event,
        serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null),
    );
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
        if out.is_empty() {
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
            tab_id,
            messages: &out,
            cursor: None,
            start_offset: Some(start_offset),
            offset: Some(offset),
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
mod tests {
    use super::*;
    use crate::agent::chat_types::{MessageBlock, RichMessage, SessionDelta};
    use crate::agent::provider::{
        AgentLaunchParams, AgentProvider, ReadinessStrategy, TaskDelivery,
    };
    use crate::lock_hold::TimedRwLock;
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::chat_send_transaction::ChatSendState;
    use crate::service::tab_chat::chatbackend_history;
    use std::io::{Read, Seek, SeekFrom};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Duration;

    struct SourcedJsonlProvider {
        root: PathBuf,
    }

    impl AgentProvider for SourcedJsonlProvider {
        fn id(&self) -> &str {
            "sourced-jsonl"
        }

        fn detect_session_id(
            &self,
            _cwd: &str,
            _exclude: &[String],
            _max_age: Option<Duration>,
        ) -> Option<String> {
            None
        }

        fn build_resume_command(
            &self,
            _session_id: &str,
            _skip_permissions: bool,
        ) -> Option<String> {
            None
        }

        fn display_name(&self) -> &str {
            "Sourced JSONL"
        }

        fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
            String::new()
        }

        fn readiness_strategy(&self) -> ReadinessStrategy {
            ReadinessStrategy::FixedDelay {
                duration: Duration::ZERO,
            }
        }

        fn task_delivery(&self) -> TaskDelivery {
            TaskDelivery::PtyInput
        }

        fn detect_from_title(&self, _: &str) -> bool {
            false
        }

        fn detect_from_output(&self, _: &str) -> bool {
            false
        }

        fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
            None
        }

        fn session_jsonl_path(&self, _: &str, session_id: &str) -> Option<PathBuf> {
            Some(self.root.join(format!("{session_id}.jsonl")))
        }

        fn parse_session_delta(&self, path: &Path, from_offset: u64) -> SessionDelta {
            let mut file = std::fs::File::open(path).unwrap();
            file.seek(SeekFrom::Start(from_offset)).unwrap();
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).unwrap();
            let mut record_start = from_offset;
            let messages = bytes
                .split_inclusive(|byte| *byte == b'\n')
                .filter_map(|line| {
                    let start = record_start;
                    record_start += line.len() as u64;
                    let value: serde_json::Value = serde_json::from_slice(line).ok()?;
                    Some(RichMessage {
                        role: value.get("role")?.as_str()?.to_string(),
                        blocks: vec![MessageBlock {
                            kind: "text".into(),
                            data: serde_json::json!({
                                "text": value.get("content")?.as_str()?
                            }),
                        }],
                        record_start: Some(start),
                        record_row: 0,
                        ..Default::default()
                    })
                })
                .collect();
            SessionDelta {
                new_offset: from_offset + bytes.len() as u64,
                messages,
            }
        }
    }

    #[derive(Clone, Copy)]
    enum SourcedEvidenceFault {
        None,
        StaleSession,
        WrongIncarnation,
        WrongBinding,
    }

    fn sourced_jsonl_send_state(
        fault: SourcedEvidenceFault,
    ) -> crate::service::chat_send_transaction::ChatSendState {
        use crate::service::tab_chat::registry::SourceObservation;
        use crate::service::tab_chat::tailer::{spawn_sourced_tailer, TailConfig};
        use std::sync::mpsc;

        const PAYLOAD: &str = "anchorless-live-evidence-ø-42";
        let session_id = "session-current";
        let binding_nonce = "binding-current";
        let root = tempfile::tempdir().unwrap();
        let (mut mux, tab_id) = MultiplexerState::new();
        {
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.backing = crate::multiplexer::types::TabBacking::Pty;
            tab.pty_id = Some("pty-current".into());
            tab.agent_type = Some("sourced-jsonl".into());
            tab.cwd = Some(root.path().to_string_lossy().into_owned());
            tab.agent_session_id = Some(session_id.into());
            tab.session_binding_nonce = Some(binding_nonce.into());
            tab.reported_session_binding_nonce = Some(binding_nonce.into());
            tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);
        }
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        {
            let gate = fixture.service.pty.lock().spawn_gate();
            crate::pty::manager::build_session(gate, None, None, 120, 40, Default::default()).map(
                |built| {
                    fixture
                        .service
                        .pty
                        .lock()
                        .register_session(Some("pty-current".into()), built)
                },
            )
        }
        .expect("the exact PTY session must be attached before admission");
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(SourcedJsonlProvider {
                root: root.path().to_path_buf(),
            }));

        let reservation = fixture
            .service
            .reserve_user_send(&tab_id, None)
            .expect("the exact PTY binding must reserve an owned send");
        let record = fixture
            .service
            .pending_delivery_payloads
            .user_sends
            .admit(&reservation, PAYLOAD, PAYLOAD, None)
            .unwrap();
        fixture
            .service
            .pending_delivery_payloads
            .user_sends
            .set_admission_wake(&tab_id, Arc::new(|| {}));
        drop(reservation);
        let accepted = fixture.service.chat_send_ledger.admit_user(&record);
        assert!(fixture
            .service
            .pending_delivery_payloads
            .user_sends
            .claim(&record, session_id)
            .unwrap());
        fixture
            .service
            .pending_delivery_payloads
            .user_sends
            .settle_cycle(&record, false)
            .unwrap();
        let captured = fixture.service.chat_send_anchor(&tab_id);
        assert!(
            captured.anchor.is_none(),
            "the exact JSONL path was absent at capture"
        );
        let absence_witness = captured.absence_witness.clone().unwrap();
        assert!(fixture
            .service
            .chat_send_ledger
            .begin_user_handoff(&record, PAYLOAD, captured));
        assert!(fixture
            .service
            .chat_send_ledger
            .mark_user_written(&accepted.send_id)
            .is_some());
        assert_eq!(
            fixture
                .service
                .chat_send_ledger
                .finish(&accepted.send_id, Ok(()))
                .unwrap()
                .state,
            ChatSendState::Unknown
        );

        let source = fixture.service.chat_registry.jsonl_source(&tab_id);
        let transcript = root.path().join(format!("{session_id}.jsonl"));
        let row = serde_json::to_vec(&serde_json::json!({
            "role": "user",
            "content": PAYLOAD,
        }))
        .unwrap();
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&transcript)
            .unwrap();
        use std::io::Write;
        file.write_all(&row).unwrap();
        file.write_all(b"\n").unwrap();
        file.sync_all().unwrap();
        let original_len = row.len() as u64 + 1;

        if matches!(fault, SourcedEvidenceFault::WrongIncarnation) {
            let SourceObservation::Present {
                incarnation: Some(first),
                ..
            } = source.observe(session_id, &transcript, None, 0)
            else {
                panic!("the appended JSONL must establish its captured incarnation");
            };
            assert_eq!(
                first.absence_witness.as_deref(),
                Some(absence_witness.as_ref())
            );
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&transcript)
                .unwrap();
            file.write_all(b" ").unwrap();
            file.write_all(&row).unwrap();
            file.write_all(b"\n").unwrap();
            file.sync_all().unwrap();
            let SourceObservation::Present {
                incarnation: Some(second),
                restart: true,
            } = source.observe(session_id, &transcript, Some(&first), original_len)
            else {
                panic!("truncate and regrow must invalidate the prior source incarnation");
            };
            assert_ne!(first.id, second.id);
            assert_eq!(second.absence_witness, None);
        }

        if matches!(fault, SourcedEvidenceFault::StaleSession) {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.agent_session_id = Some("session-next".into());
            tab.session_binding_nonce = Some("binding-next".into());
            tab.reported_session_binding_nonce = Some("binding-next".into());
        }

        let sink_nonce = if matches!(fault, SourcedEvidenceFault::WrongBinding) {
            "binding-wrong"
        } else {
            binding_nonce
        };
        let (event_tx, event_rx) = mpsc::sync_channel(1);
        let listener = fixture
            .ui
            .listen(&format!("tab-chat-msg-{tab_id}"), move |payload| {
                let _ = event_tx.try_send(payload.clone());
            });
        let sink = make_flatten_sink(
            fixture.service.lifecycle_journal.clone(),
            &fixture.service,
            fixture.service.mux.clone(),
            0,
            Some(session_id.into()),
            Some(sink_nonce.into()),
            Some(fixture.service.chat_send_ledger.clone()),
        );
        let tailer = spawn_sourced_tailer(
            TailConfig {
                tab_id: tab_id.clone(),
                provider_id: "sourced-jsonl".into(),
                cwd: root.path().to_string_lossy().into_owned(),
                session_id: session_id.into(),
                start_offset: 0,
            },
            Some(source),
            fixture.service.agent_registry.clone(),
            sink,
        );
        let event = event_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("the sourced tailer must emit through FlattenDeltaSink");
        tailer.shutdown();
        fixture.ui.unlisten(listener);
        assert_eq!(event["messages"][0]["type"], "user");
        assert_eq!(event["messages"][0]["content"], PAYLOAD);

        let status = fixture
            .service
            .chat_send_ledger
            .status(&accepted.send_id)
            .unwrap();
        match fault {
            SourcedEvidenceFault::None => {
                assert!(!absence_witness.is_empty());
                assert_eq!(status.state, ChatSendState::Sent);
            }
            _ => assert_eq!(status.state, ChatSendState::Unknown),
        }
        status.state
    }

    #[test]
    fn sourced_jsonl_confirmation_traverses_tailer_flattening_and_ledger_gates() {
        assert_eq!(
            sourced_jsonl_send_state(SourcedEvidenceFault::None),
            ChatSendState::Sent
        );
        assert_eq!(
            sourced_jsonl_send_state(SourcedEvidenceFault::StaleSession),
            ChatSendState::Unknown
        );
        assert_eq!(
            sourced_jsonl_send_state(SourcedEvidenceFault::WrongIncarnation),
            ChatSendState::Unknown
        );
        assert_eq!(
            sourced_jsonl_send_state(SourcedEvidenceFault::WrongBinding),
            ChatSendState::Unknown
        );
    }

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

    /// Missing-store inverse guard: an unanchored send still reaches `Sent`
    /// from an attributable bound-session turn, with no readiness machinery.
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
            ChatSendState::Unknown,
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
            None,
        );
        assert!(stale.is_empty(), "stale-scope turns cannot confirm");

        let confirmed = current_binding_turn_confirmations(
            &mux,
            Some(&ledger),
            &tab_id,
            Some("sess-live"),
            Some("nonce-live"),
            &user_turn,
            None,
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
        let inbound = codeterm_wire::domios::DomiosFrame {
            opening_tag: codeterm_wire::domios::build_opening_tag(&[
                ("from", "tab"),
                ("label", "Alice"),
                ("tab", "tab-a"),
            ]),
            body: "hello".into(),
            suffix: String::new(),
        }
        .to_message();

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

    #[test]
    fn conversation_evidence_uses_injected_journal_and_exact_live_binding() {
        let (mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        fixture.service.open_lifecycle_epoch(&tab_id).unwrap();
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.agent_session_id = Some("own-session".into());
            tab.session_binding_nonce = Some("own-nonce".into());
        }
        let before = fixture.service.lifecycle_journal.load().records;
        let messages = [ChatMsg {
            kind: "user".into(),
            content: "opaque-user-turn".into(),
            ..Default::default()
        }];
        let observe = |session, nonce| {
            note_current_session_conversation(
                &fixture.service.lifecycle_journal,
                &fixture.service.mux,
                Some(&fixture.service.chat_send_ledger),
                &tab_id,
                Some(session),
                Some(nonce),
                &messages,
            )
        };
        observe("foreign-session", "own-nonce");
        observe("own-session", "foreign-nonce");
        assert_eq!(fixture.service.lifecycle_journal.load().records, before);
        observe("own-session", "own-nonce");
        observe("own-session", "own-nonce");
        let records = fixture.service.lifecycle_journal.load().records;
        assert_eq!(records.len(), before.len() + 1);
        assert!(matches!(&records.last().unwrap(),
            crate::session::lifecycle::JournalRecord::Transition { entry }
                if matches!(&entry.transition,
                    crate::session::lifecycle::LifecycleTransition::ConversationStarted { session_id }
                        if session_id == "own-session"
                )
        ));
    }
}
