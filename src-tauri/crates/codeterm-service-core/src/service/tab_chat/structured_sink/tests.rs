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
