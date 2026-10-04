    use super::*;
    use parking_lot::Mutex;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier,
    };

    #[test]
    fn duplicate_ready_triggers_atomically_claim_one_pending_record_once() {
        let record =
            PendingDeliveryRecord::new("digest".into(), PendingDeliveryChannel::InteractiveInput)
                .with_send_id("send-1".into());
        let record_id = record.record_id().to_string();
        let pending = Arc::new(Mutex::new(Some(record)));
        let writes = Arc::new(AtomicUsize::new(0));
        let ready = Arc::new(Barrier::new(3));
        let triggers = (0..2)
            .map(|_| {
                let pending = pending.clone();
                let writes = writes.clone();
                let ready = ready.clone();
                let record_id = record_id.clone();
                std::thread::spawn(move || {
                    ready.wait();
                    if take_parked_user_send(&mut pending.lock(), &record_id, Some("send-1"))
                        .is_some()
                    {
                        writes.fetch_add(1, Ordering::SeqCst);
                    }
                })
            })
            .collect::<Vec<_>>();
        ready.wait();
        for trigger in triggers {
            trigger.join().unwrap();
        }

        assert_eq!(writes.load(Ordering::SeqCst), 1);
        assert!(pending.lock().is_none());
    }

    #[test]
    fn parked_send_claim_requires_the_exact_pending_record_id() {
        let mut pending = Some(
            PendingDeliveryRecord::new("digest".into(), PendingDeliveryChannel::InteractiveInput)
                .with_send_id("send-1".into()),
        );
        let record_id = pending.as_ref().unwrap().record_id().to_string();

        assert!(take_parked_user_send(&mut pending, "other-record", Some("send-1")).is_none());
        assert!(take_parked_user_send(&mut pending, &record_id, Some("send-2")).is_none());
        assert!(take_parked_user_send(&mut pending, &record_id, Some("send-1")).is_some());
    }

    #[test]
    fn legacy_pending_record_without_send_id_is_deliverable_by_record_id() {
        let legacy: PendingDeliveryRecord = serde_json::from_str(
            r#"{"content_digest":"digest","channel":"interactive_input","state":"pending","attempt_provenance":null}"#,
        )
        .unwrap();
        let mut pending = Some(legacy);
        let record_id = pending.as_ref().unwrap().record_id().to_string();

        assert_eq!(pending.as_ref().unwrap().send_id(), None);
        assert!(take_parked_user_send(&mut pending, &record_id, None).is_some());
        assert!(pending.is_none());
    }

    #[test]
    fn prewrite_failure_restores_but_postwrite_failure_does_not() {
        let record =
            PendingDeliveryRecord::new("digest".into(), PendingDeliveryChannel::InteractiveInput);
        let record_id = record.record_id().to_string();
        let mut pending = None;

        assert!(return_after_send_failure(&mut pending, record, false).is_ok());
        assert_eq!(pending.as_ref().unwrap().record_id(), record_id);

        let claimed = pending.take().unwrap();
        assert!(return_after_send_failure(&mut pending, claimed, true).is_err());
        assert!(pending.is_none());
    }

    #[test]
    fn unconfirmed_send_is_not_claimed_for_another_write() {
        let mut pending = Some(
            PendingDeliveryRecord::new("digest".into(), PendingDeliveryChannel::InteractiveInput)
                .with_send_id("send-1".into()),
        );
        pending.as_mut().unwrap().mark_unconfirmed();
        let record_id = pending.as_ref().unwrap().record_id().to_string();

        assert!(take_parked_user_send(&mut pending, &record_id, Some("send-1")).is_none());
        assert!(pending.is_some());
    }

use crate::service::chat_send_transaction::{run_chat_send, ChatSendState, ChatSendStatus};
use crate::service::delivery_error::{DeliveryError, SendOrigin};
use crate::service::pending_delivery_payloads::user_sends::UserSendStore;
use crate::service::test_fixture::ServiceFixture;
use std::sync::mpsc;
use std::time::Duration;
use tauri::{Listener, Manager};

const FIRST_PAYLOAD: &str = " \topaque-a-41c9\r\n\u{03bb}\t ";
const SECOND_PAYLOAD: &str = "\topaque-b-8d27\n\u{754c}  ";
const REFUSAL: &str = "ProviderInputNotReady: opaque-readiness-boundary";
const TEST_DEADLINE: Duration = Duration::from_secs(10);

pub(crate) fn failed_launch_fixture() -> (ServiceFixture, String) {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("opaque-provider".into());
    tab.pty_id = Some("opaque-pty".into());
    tab.agent_session_id = Some("opaque-session".into());
    tab.session_binding_nonce = Some("opaque-current-nonce".into());
    tab.reported_session_binding_nonce = Some("opaque-old-nonce".into());
    tab.boot_stage = Some(codeterm_types::pty::BootStage::SpawnFailed);
    tab.boot_failure_reason = Some(REFUSAL.into());
    let fixture = ServiceFixture::new(mux);
    let session: crate::session::SessionState = Arc::new(Mutex::new(
        crate::session::SessionStore::for_test_dir(fixture.temp.path().join("sessions")),
    ));
    assert!(fixture._app.manage(session));
    assert!(fixture._app.manage(Arc::new(fixture.service.clone()) as crate::service::ServiceState));
    (fixture, tab_id)
}

fn admit_current_ledger(
    svc: &crate::service::ServiceContext,
    tab_id: &str,
    text: &str,
    client_token: &str,
) -> ChatSendStatus {
    let evidence = svc.chat_send_anchor(tab_id);
    let accepted = svc
        .chat_send_ledger
        .try_start_scoped(tab_id, text, evidence.anchor, evidence.live_binding)
        .unwrap();
    svc.chat_send_ledger
        .attach_client_token(&accepted.send_id, client_token.into())
        .unwrap()
}

fn refuse_and_park_current_send(
    svc: &crate::service::ServiceContext,
    accepted: ChatSendStatus,
    payload: &str,
) {
    let tab_id = accepted.tab_id.clone();
    let send_id = accepted.send_id.clone();
    let status = run_chat_send(&svc.chat_send_ledger, accepted, payload, |text| {
        let error = svc.send_chat_text_claimed(&tab_id, text, None).unwrap_err();
        assert!(matches!(&error, DeliveryError::TabBusyRefused { detail, .. } if detail == REFUSAL));
        assert!(!error.bytes_written());
        assert!(svc.pty.lock().get_session("opaque-pty").is_none());
        svc.park_refused_user_send(&tab_id, &send_id, text, &error);
        error.into_surfaced(SendOrigin::User)
    });
    assert_eq!(status.state, ChatSendState::Unknown);
    assert_eq!(status.error, None);
}

fn assert_reopened_originals(
    fixture: &ServiceFixture,
    accepted: &[ChatSendStatus; 2],
) {
    assert_ne!(accepted[0].send_id, accepted[1].send_id);
    assert!(accepted[0].sequence < accepted[1].sequence);
    let reopened = UserSendStore::open(fixture.temp.path()).unwrap();
    let records = reopened.records(&accepted[0].tab_id).unwrap();
    assert_eq!(records.iter().map(|record| &record.send_id).collect::<Vec<_>>(),
        vec![&accepted[0].send_id, &accepted[1].send_id]);
    assert!(records[0].acceptance_sequence < records[1].acceptance_sequence);
    let payloads = accepted.each_ref().map(|status| reopened.original(&status.tab_id, &status.send_id));
    assert_eq!(
        payloads[0].as_deref(),
        Ok(FIRST_PAYLOAD),
        "the first accepted zero-write send must remain addressable after reopen"
    );
    assert_eq!(
        payloads[1].as_deref(),
        Ok(SECOND_PAYLOAD),
        "the second accepted zero-write send must remain addressable after reopen"
    );
}

fn start_current_chat_send(
    fixture: &ServiceFixture,
    tab_id: &str,
    payload: &str,
    client_token: &str,
) -> ChatSendStatus {
    let (tx, rx) = mpsc::sync_channel(1);
    let expected_tab = tab_id.to_string();
    let listener = fixture._app.listen("chat-send-status", move |event| {
        let value: serde_json::Value = serde_json::from_str(event.payload()).unwrap();
        if value["tab_id"] == expected_tab {
            let _ = tx.try_send(value);
        }
    });
    let accepted = fixture
        .service
        .start_chat_send(tab_id, payload.into(), None, Some(client_token.into()))
        .unwrap();
    let published = rx.recv_timeout(TEST_DEADLINE);
    fixture._app.unlisten(listener);
    let published = published.expect("the current service completed admission and parking");
    assert_eq!(published["send_id"], accepted.send_id);
    assert!(matches!(published["state"].as_str(), Some("accepted" | "unknown")));
    assert!(published["error"].is_null());
    assert_eq!(accepted.correlation_id.as_deref(), Some(client_token));
    assert!(fixture.service.pty.lock().get_session("opaque-pty").is_none());
    assert_eq!(fixture.service.mux.read().get_tab(tab_id).unwrap().boot_stage,
        Some(codeterm_types::pty::BootStage::SpawnFailed));
    assert!(!fixture.service.user_authority_is_ready(&fixture.service.user_send_authority(tab_id).unwrap()));
    accepted
}

#[test]
#[cfg_attr(not(windows), ignore = "requires the shared interactive desktop ServiceContext fixture")]
fn spawn_failed_two_accepted_user_sends_retain_originals_after_reopen() {
    let (fixture, tab_id) = failed_launch_fixture();
    let first = start_current_chat_send(&fixture, &tab_id, FIRST_PAYLOAD, "opaque-client-a");
    assert_eq!(
        fixture.service.pending_delivery_payloads.user_sends.original(&tab_id, &first.send_id).unwrap(),
        FIRST_PAYLOAD,
        "first admission must actually reach the real payload store"
    );
    let second = start_current_chat_send(&fixture, &tab_id, SECOND_PAYLOAD, "opaque-client-b");

    assert_reopened_originals(&fixture, &[first, second]);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires the shared interactive desktop ServiceContext fixture")]
fn reverse_transport_completion_retains_both_accepted_originals_after_reopen() {
    let (fixture, tab_id) = failed_launch_fixture();
    let first = admit_current_ledger(&fixture.service, &tab_id, FIRST_PAYLOAD, "opaque-client-a");
    let second = admit_current_ledger(&fixture.service, &tab_id, SECOND_PAYLOAD, "opaque-client-b");
    let (waiting_tx, waiting_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let svc = fixture.service.clone();
    let delayed = first.clone();
    let worker = std::thread::spawn(move || {
        waiting_tx.send(()).unwrap();
        release_rx.recv_timeout(TEST_DEADLINE).unwrap();
        refuse_and_park_current_send(&svc, delayed, FIRST_PAYLOAD);
    });
    waiting_rx.recv_timeout(TEST_DEADLINE).unwrap();
    refuse_and_park_current_send(&fixture.service, second.clone(), SECOND_PAYLOAD);
    release_tx.send(()).unwrap();
    crate::service::pty_io::test_support::join(worker, "first refused send").unwrap();

    assert_reopened_originals(&fixture, &[first, second]);
}

type SubmitBoundary = Arc<dyn Fn(&UserSendRecord, &str) -> Result<(), DeliveryError> + Send + Sync>;
static SUBMIT_BOUNDARIES: std::sync::OnceLock<Mutex<std::collections::HashMap<String, SubmitBoundary>>> = std::sync::OnceLock::new();

pub(crate) struct UserSubmitBoundary(String);

impl UserSubmitBoundary {
    pub(crate) fn install(tab_id: &str, boundary: SubmitBoundary) -> Self {
        SUBMIT_BOUNDARIES.get_or_init(Default::default).lock().insert(tab_id.to_string(), boundary);
        Self(tab_id.to_string())
    }
}

impl Drop for UserSubmitBoundary {
    fn drop(&mut self) {
        SUBMIT_BOUNDARIES.get_or_init(Default::default).lock().remove(&self.0);
    }
}

pub(super) fn submit_boundary(tab_id: &str) -> Option<SubmitBoundary> {
    SUBMIT_BOUNDARIES.get_or_init(Default::default).lock().get(tab_id).cloned()
}

type BootstrapBoundary = Arc<dyn Fn(&str) -> Result<(), DeliveryError> + Send + Sync>;
static BOOTSTRAP_BOUNDARIES: std::sync::OnceLock<Mutex<std::collections::HashMap<String, BootstrapBoundary>>> = std::sync::OnceLock::new();

struct BootstrapSubmitBoundary(String);

impl BootstrapSubmitBoundary {
    fn install(tab_id: &str, boundary: BootstrapBoundary) -> Self {
        BOOTSTRAP_BOUNDARIES.get_or_init(Default::default).lock().insert(tab_id.to_string(), boundary);
        Self(tab_id.to_string())
    }
}

impl Drop for BootstrapSubmitBoundary {
    fn drop(&mut self) {
        BOOTSTRAP_BOUNDARIES.get_or_init(Default::default).lock().remove(&self.0);
    }
}

pub(super) fn bootstrap_boundary(tab_id: &str) -> Option<BootstrapBoundary> {
    BOOTSTRAP_BOUNDARIES.get_or_init(Default::default).lock().get(tab_id).cloned()
}

struct QueueProvider {
    root: Option<std::path::PathBuf>,
    paste_limit: Option<usize>,
}

impl crate::agent::provider::AgentProvider for QueueProvider {
    fn id(&self) -> &str { "opaque-provider" }
    fn display_name(&self) -> &str { "Opaque Provider" }
    fn build_launch_command(&self, _: &crate::agent::provider::AgentLaunchParams) -> String { String::new() }
    fn readiness_strategy(&self) -> crate::agent::provider::ReadinessStrategy {
        crate::agent::provider::ReadinessStrategy::FixedDelay { duration: Duration::ZERO }
    }
    fn task_delivery(&self) -> crate::agent::provider::TaskDelivery { crate::agent::provider::TaskDelivery::PtyInput }
    fn detect_from_title(&self, _: &str) -> bool { false }
    fn detect_from_output(&self, _: &str) -> bool { false }
    fn session_file_path(&self, _: &str, session: &str) -> Option<std::path::PathBuf> {
        self.root.as_ref().map(|root| root.join(format!("{session}.jsonl")))
    }
    fn session_jsonl_path(&self, cwd: &str, session: &str) -> Option<std::path::PathBuf> {
        self.session_file_path(cwd, session)
    }
    fn session_exists(&self, _: &str, _: &str) -> bool { true }
    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> { None }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> { None }
    fn paste_limit_bytes(&self) -> Option<usize> { self.paste_limit }
    fn parse_session_delta(&self, path: &std::path::Path, from_offset: u64) -> crate::agent::chat_types::SessionDelta {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = std::fs::File::open(path).unwrap();
        file.seek(SeekFrom::Start(from_offset)).unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        let messages = bytes.split(|byte| *byte == b'\n').filter_map(|line| {
            let value: serde_json::Value = serde_json::from_slice(line).ok()?;
            Some(crate::agent::chat_types::RichMessage {
                role: "user".into(),
                blocks: vec![crate::agent::chat_types::MessageBlock {
                    kind: "text".into(), data: serde_json::json!({ "text": value["content"].as_str()? }),
                }],
                ..Default::default()
            })
        }).collect();
        crate::agent::chat_types::SessionDelta { new_offset: from_offset + bytes.len() as u64, messages }
    }
}

pub(crate) fn queue_fixture(anchor: bool, paste_limit: Option<usize>) -> (ServiceFixture, String) {
    let (fixture, tab) = failed_launch_fixture();
    let root = fixture.temp.path().join("transcripts");
    std::fs::create_dir(&root).unwrap();
    if anchor { std::fs::write(root.join("opaque-session.jsonl"), b"").unwrap(); }
    fixture.service.agent_registry.register_provider(Box::new(QueueProvider {
        root: Some(root), paste_limit,
    }));
    (fixture, tab)
}

pub(crate) fn mark_ready(fixture: &ServiceFixture, tab_id: &str) {
    let mut mux = fixture.service.mux.write();
    let tab = mux.get_tab_mut(tab_id).unwrap();
    tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);
    tab.boot_failure_reason = None;
}

pub(crate) fn live_turn(fixture: &ServiceFixture, tab: &str, session: &str, nonce: &str, id: &str, text: &str) -> Vec<ChatSendStatus> {
    crate::service::tab_chat::structured_sink::current_binding_turn_confirmations(
        &fixture.service.mux, Some(&fixture.service.chat_send_ledger), tab, Some(session), Some(nonce),
        &[crate::agent::chat_types::ChatMsg { id: id.into(), kind: "user".into(), content: text.into(), ..Default::default() }],
    )
}

fn append_turn(fixture: &ServiceFixture, text: &str) {
    use std::io::Write;
    let path = fixture.temp.path().join("transcripts").join("opaque-session.jsonl");
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
    writeln!(file, "{}", serde_json::json!({ "content": text })).unwrap();
}

fn settled_listener(fixture: &ServiceFixture, tab: &str) -> (tauri::EventId, mpsc::Receiver<serde_json::Value>) {
    let (tx, rx) = mpsc::channel();
    let tab = tab.to_string();
    let listener = fixture._app.listen("chat-send-status", move |event| {
        let value: serde_json::Value = serde_json::from_str(event.payload()).unwrap();
        if value["tab_id"] == tab && value["state"] != "accepted" { let _ = tx.send(value); }
    });
    (listener, rx)
}

#[test]
#[cfg_attr(not(windows), ignore = "requires the shared interactive desktop ServiceContext fixture")]
fn ordered_owner_joins_exact_append_evidence_and_cycle_settlement_before_tail() {
    let (fixture, tab) = queue_fixture(true, None);
    append_turn(&fixture, "opaque-equal");
    let first = start_current_chat_send(&fixture, &tab, "opaque-equal", "client-a");
    let second = start_current_chat_send(&fixture, &tab, "opaque-equal", "client-b");
    let (writes_tx, writes_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let _boundary = UserSubmitBoundary::install(&tab, Arc::new(move |record, text| {
        writes_tx.send((record.send_id.clone(), text.to_string())).unwrap();
        release_rx.lock().recv_timeout(TEST_DEADLINE).unwrap();
        Ok(())
    }));
    mark_ready(&fixture, &tab);
    fixture.service.wake_parked_user_send(&tab);
    fixture.service.wake_parked_user_send(&tab);
    assert_eq!(writes_rx.recv_timeout(TEST_DEADLINE).unwrap(), (first.send_id.clone(), "opaque-equal".into()));
    assert!(live_turn(&fixture, &tab, "other-session", "opaque-current-nonce", "wrong-session", "opaque-equal").is_empty());
    assert!(live_turn(&fixture, &tab, "opaque-session", "old-nonce", "wrong-nonce", "opaque-equal").is_empty());
    fixture.service.reconcile_chat_send_evidence(&tab);
    assert!(!fixture.service.chat_send_ledger.user_confirmed(&first.send_id));
    assert!(live_turn(&fixture, &tab, "opaque-session", "opaque-current-nonce", "historical-turn", "opaque-equal").is_empty());
    append_turn(&fixture, "opaque-equal");
    fixture.service.reconcile_chat_send_evidence(&tab);
    assert!(fixture.service.chat_send_ledger.user_confirmed(&first.send_id));
    let records = fixture.service.pending_delivery_payloads.user_sends.records(&tab).unwrap();
    assert_eq!(records[0].state, UserSendState::Claimed);
    assert!(!records[0].cycle_settled);
    assert_eq!(records[1].state, UserSendState::Queued);
    assert!(matches!(writes_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    release_tx.send(()).unwrap();
    assert_eq!(writes_rx.recv_timeout(TEST_DEADLINE).unwrap(), (second.send_id.clone(), "opaque-equal".into()));
    fixture.service.reconcile_chat_send_evidence(&tab);
    assert!(!fixture.service.chat_send_ledger.user_confirmed(&second.send_id));
    append_turn(&fixture, "opaque-equal");
    fixture.service.reconcile_chat_send_evidence(&tab);
    assert!(fixture.service.chat_send_ledger.user_confirmed(&second.send_id));
    let (listener, settled) = settled_listener(&fixture, &tab);
    release_tx.send(()).unwrap();
    assert_eq!(settled.recv_timeout(TEST_DEADLINE).unwrap()["send_id"], second.send_id);
    fixture._app.unlisten(listener);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires the shared interactive desktop ServiceContext fixture")]
fn anchorless_first_send_materializes_history_and_live_turn_waits_for_cycle_settlement() {
    let (fixture, tab) = queue_fixture(false, None);
    let first = start_current_chat_send(&fixture, &tab, "opaque-equal", "client-a");
    let second = start_current_chat_send(&fixture, &tab, "opaque-equal", "client-b");
    assert!(fixture.service.chat_send_anchor(&tab).anchor.is_none());
    let (writes_tx, writes_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let _boundary = UserSubmitBoundary::install(&tab, Arc::new(move |record, _| {
        writes_tx.send(record.send_id.clone()).unwrap();
        release_rx.lock().recv_timeout(TEST_DEADLINE).unwrap();
        Ok(())
    }));
    mark_ready(&fixture, &tab);
    fixture.service.wake_parked_user_send(&tab);
    assert_eq!(writes_rx.recv_timeout(TEST_DEADLINE).unwrap(), first.send_id);
    append_turn(&fixture, "opaque-equal");
    let confirmed = live_turn(&fixture, &tab, "opaque-session", "opaque-current-nonce", "turn-a", "opaque-equal");
    assert_eq!(confirmed.len(), 1);
    assert_eq!(confirmed[0].send_id, first.send_id);
    assert!(live_turn(&fixture, &tab, "opaque-session", "opaque-current-nonce", "turn-a", "opaque-equal").is_empty());
    assert!(matches!(writes_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    assert!(!fixture.service.pending_delivery_payloads.user_sends.head(&tab).unwrap().unwrap().cycle_settled);
    release_tx.send(()).unwrap();
    assert_eq!(writes_rx.recv_timeout(TEST_DEADLINE).unwrap(), second.send_id);
    assert!(!fixture.service.chat_send_ledger.user_confirmed(&second.send_id));
    assert!(live_turn(&fixture, &tab, "opaque-session", "opaque-current-nonce", "turn-a", "opaque-equal").is_empty());
    assert!(!fixture.service.chat_send_ledger.user_confirmed(&second.send_id));
    append_turn(&fixture, "opaque-equal");
    fixture.service.reconcile_chat_send_evidence(&tab);
    let (listener, settled) = settled_listener(&fixture, &tab);
    release_tx.send(()).unwrap();
    assert_eq!(settled.recv_timeout(TEST_DEADLINE).unwrap()["send_id"], second.send_id);
    fixture._app.unlisten(listener);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires the shared interactive desktop ServiceContext fixture")]
fn postwrite_uncertainty_duplicate_wakes_and_rebind_preserve_tail_without_replay() {
    let (fixture, tab) = queue_fixture(true, None);
    let first = start_current_chat_send(&fixture, &tab, FIRST_PAYLOAD, "client-a");
    let second = start_current_chat_send(&fixture, &tab, SECOND_PAYLOAD, "client-b");
    let writes = Arc::new(AtomicUsize::new(0));
    let count = writes.clone();
    let _boundary = UserSubmitBoundary::install(&tab, Arc::new(move |_, _| {
        count.fetch_add(1, Ordering::SeqCst);
        Err(DeliveryError::SubmitUnconfirmed { tab: "opaque-tab".into(), detail: "opaque-uncertainty".into() })
    }));
    let (listener, settled) = settled_listener(&fixture, &tab);
    mark_ready(&fixture, &tab);
    fixture.service.wake_parked_user_send(&tab);
    assert_eq!(settled.recv_timeout(TEST_DEADLINE).unwrap()["state"], "unknown");
    fixture._app.unlisten(listener);
    fixture.service.drain_user_owner(&tab).unwrap();
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    fixture.service.mux.write().get_tab_mut(&tab).unwrap().pty_id = Some("other-pty".into());
    append_turn(&fixture, FIRST_PAYLOAD);
    fixture.service.reconcile_chat_send_evidence(&tab);
    assert!(!fixture.service.chat_send_ledger.user_confirmed(&first.send_id));
    fixture.service.drain_user_owner(&tab).unwrap();
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    let reopened = UserSendStore::open(fixture.temp.path()).unwrap();
    let records = reopened.records(&tab).unwrap();
    assert_eq!(records[0].state, UserSendState::Uncertain);
    assert_eq!(records[1].send_id, second.send_id);
    assert_reopened_originals(&fixture, &[first, second]);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires the shared interactive desktop ServiceContext fixture")]
fn held_persistence_keeps_mux_ledger_and_unrelated_admission_available_without_false_acceptance() {
    let (fixture, tab) = queue_fixture(true, None);
    let (held_tx, held_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Arc::new(Mutex::new(release_rx));
    fixture.service.pending_delivery_payloads.user_sends.set_persistence_boundary(Some(Arc::new(move |stage| {
        if stage == "index" {
            held_tx.send(()).unwrap();
            release_rx.lock().recv_timeout(TEST_DEADLINE).unwrap();
        }
        Ok(())
    })));
    let svc = fixture.service.clone();
    let reserved = svc.reserve_user_send(&tab, Some("client-a".into())).unwrap();
    let send_id = reserved.send_id.clone();
    let (accepted_tx, accepted_rx) = mpsc::channel();
    let admission = std::thread::spawn(move || {
        let result = svc.prepare_and_admit_user_send(reserved, FIRST_PAYLOAD.into(), false);
        accepted_tx.send(result.is_ok()).unwrap();
    });
    held_rx.recv_timeout(TEST_DEADLINE).unwrap();
    assert!(matches!(accepted_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    let (probe_tx, probe_rx) = mpsc::channel();
    let svc = fixture.service.clone();
    let probe_tab = tab.clone();
    let probe_id = send_id.clone();
    let probe = std::thread::spawn(move || {
        let mux_available = svc.mux.try_write().is_some();
        let unpublished = svc.chat_send_ledger.status(&probe_id).is_none();
        let reserved = svc.reserve_user_send(&probe_tab, Some("client-b".into())).unwrap();
        probe_tx.send((mux_available, unpublished, reserved.ledger_sequence)).unwrap();
    });
    let observed = probe_rx.recv_timeout(TEST_DEADLINE);
    release_tx.send(()).unwrap();
    let (mux_available, unpublished, sequence) = observed.unwrap();
    assert!(mux_available);
    assert!(unpublished);
    assert!(sequence > 0);
    probe.join().unwrap();
    assert!(accepted_rx.recv_timeout(TEST_DEADLINE).unwrap());
    admission.join().unwrap();
    fixture.service.pending_delivery_payloads.user_sends.set_persistence_boundary(None);
    assert_eq!(fixture.service.pending_delivery_payloads.user_sends.original(&tab, &send_id).unwrap(), FIRST_PAYLOAD);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires the shared interactive desktop ServiceContext fixture")]
fn close_during_durable_claim_revokes_before_any_writer_handoff_and_preserves_both() {
    let (fixture, tab) = queue_fixture(true, None);
    let first = start_current_chat_send(&fixture, &tab, FIRST_PAYLOAD, "client-a");
    let second = start_current_chat_send(&fixture, &tab, SECOND_PAYLOAD, "client-b");
    let writes = Arc::new(AtomicUsize::new(0));
    let count = writes.clone();
    let _boundary = UserSubmitBoundary::install(&tab, Arc::new(move |_, _| { count.fetch_add(1, Ordering::SeqCst); Ok(()) }));
    let (held_tx, held_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let once = Arc::new(std::sync::atomic::AtomicBool::new(false));
    fixture.service.pending_delivery_payloads.user_sends.set_persistence_boundary(Some(Arc::new(move |stage| {
        if stage == "index" && !once.swap(true, Ordering::SeqCst) {
            held_tx.send(()).unwrap();
            release_rx.lock().recv_timeout(TEST_DEADLINE).unwrap();
        }
        Ok(())
    })));
    mark_ready(&fixture, &tab);
    fixture.service.wake_parked_user_send(&tab);
    held_rx.recv_timeout(TEST_DEADLINE).unwrap();
    fixture.service.prepare_tab_close(&tab, crate::multiplexer::types::TabCloseReason::User).unwrap();
    assert!(fixture.service.pending_delivery_payloads.user_sends.tab(&tab).revoked.load(Ordering::Acquire));
    let (listener, settled) = settled_listener(&fixture, &tab);
    release_tx.send(()).unwrap();
    assert_eq!(settled.recv_timeout(TEST_DEADLINE).unwrap()["send_id"], first.send_id);
    fixture._app.unlisten(listener);
    fixture.service.pending_delivery_payloads.user_sends.set_persistence_boundary(None);
    fixture.service.wake_parked_user_send(&tab);
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    assert_reopened_originals(&fixture, &[first, second]);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires the shared interactive desktop ServiceContext fixture")]
fn production_preparation_archives_exact_owned_body_and_preserves_it_after_pad_removal() {
    let (fixture, tab) = queue_fixture(true, Some(8));
    let ticket = fixture.service.reserve_user_send(&tab, Some("opaque-client".into())).unwrap();
    let result = fixture.service.prepare_and_admit_user_send(ticket, FIRST_PAYLOAD.into(), false).unwrap();
    let body_path = std::path::PathBuf::from(result.pad_file.unwrap());
    let expected = fixture.service.ensure_agent_pad(&tab).unwrap().path().to_path_buf();
    assert!(!body_path.starts_with(&expected));
    assert_eq!(std::fs::read(&body_path).unwrap(), FIRST_PAYLOAD.as_bytes());
    fixture.service.revoke_agent_pad(&tab).unwrap();
    let reopened = UserSendStore::open(fixture.temp.path()).unwrap();
    let record = reopened.records(&tab).unwrap().remove(0);
    assert_eq!(reopened.original(&tab, &result.status.send_id).unwrap(), FIRST_PAYLOAD);
    assert_eq!(std::fs::read(reopened.body_path(&record).unwrap().unwrap()).unwrap(), FIRST_PAYLOAD.as_bytes());
    assert!(reopened.prepared(&record).unwrap().ends_with(codeterm_paths::strip_verbatim_prefix(body_path.to_str().unwrap())));
}

#[test]
#[cfg_attr(not(windows), ignore = "requires the shared interactive desktop ServiceContext fixture")]
fn reversed_service_preparation_keeps_intake_order_before_first_handoff() {
    let (fixture, tab) = queue_fixture(false, None);
    let first = fixture.service.reserve_user_send(&tab, Some("client-a".into())).unwrap();
    let second = fixture.service.reserve_user_send(&tab, Some("client-b".into())).unwrap();
    let (writes_tx, writes_rx) = mpsc::channel();
    let _boundary = UserSubmitBoundary::install(&tab, Arc::new(move |record, _| {
        writes_tx.send(record.send_id.clone()).unwrap();
        Err(DeliveryError::SubmitUnconfirmed { tab: record.authority.tab_id.clone(), detail: "opaque-uncertainty".into() })
    }));
    mark_ready(&fixture, &tab);
    let b = fixture.service.prepare_and_admit_user_send(second, SECOND_PAYLOAD.into(), false).unwrap();
    fixture.service.drain_user_owner(&tab).unwrap();
    assert!(matches!(writes_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    let a = fixture.service.prepare_and_admit_user_send(first, FIRST_PAYLOAD.into(), false).unwrap();
    assert_eq!(writes_rx.recv_timeout(TEST_DEADLINE).unwrap(), a.status.send_id);
    assert_reopened_originals(&fixture, &[a.status, b.status]);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires the shared interactive desktop ServiceContext fixture")]
fn anchorless_equal_text_requires_two_distinct_live_turn_ids() {
    let (fixture, tab) = queue_fixture(false, None);
    let first = start_current_chat_send(&fixture, &tab, "opaque-equal", "client-a");
    let second = start_current_chat_send(&fixture, &tab, "opaque-equal", "client-b");
    let (writes_tx, writes_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let _boundary = UserSubmitBoundary::install(&tab, Arc::new(move |record, _| {
        writes_tx.send(record.send_id.clone()).unwrap();
        release_rx.lock().recv_timeout(TEST_DEADLINE).unwrap();
        Ok(())
    }));
    mark_ready(&fixture, &tab);
    fixture.service.wake_parked_user_send(&tab);
    assert_eq!(writes_rx.recv_timeout(TEST_DEADLINE).unwrap(), first.send_id);
    assert_eq!(live_turn(&fixture, &tab, "opaque-session", "opaque-current-nonce", "turn-a", "opaque-equal")[0].send_id, first.send_id);
    release_tx.send(()).unwrap();
    assert_eq!(writes_rx.recv_timeout(TEST_DEADLINE).unwrap(), second.send_id);
    assert!(fixture.service.chat_send_anchor(&tab).anchor.is_none());
    assert!(live_turn(&fixture, &tab, "opaque-session", "opaque-current-nonce", "turn-a", "opaque-equal").is_empty());
    assert!(!fixture.service.chat_send_ledger.user_confirmed(&second.send_id));
    assert_eq!(live_turn(&fixture, &tab, "opaque-session", "opaque-current-nonce", "turn-b", "opaque-equal")[0].send_id, second.send_id);
    let (listener, settled) = settled_listener(&fixture, &tab);
    release_tx.send(()).unwrap();
    assert_eq!(settled.recv_timeout(TEST_DEADLINE).unwrap()["send_id"], second.send_id);
    fixture._app.unlisten(listener);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires the shared interactive desktop ServiceContext fixture")]
fn refused_durable_service_admission_never_publishes_acceptance_or_evicts_first() {
    let (fixture, tab) = queue_fixture(true, None);
    let accepted = start_current_chat_send(&fixture, &tab, FIRST_PAYLOAD, "client-a");
    fixture.service.pending_delivery_payloads.user_sends.set_persistence_boundary(Some(Arc::new(|stage| {
        if stage == "index" { Err("opaque-index-error".into()) } else { Ok(()) }
    })));
    let reserved = fixture.service.reserve_user_send(&tab, Some("client-b".into())).unwrap();
    let refused_id = reserved.send_id.clone();
    let error = fixture.service.prepare_and_admit_user_send(reserved, SECOND_PAYLOAD.into(), false).err().unwrap();
    assert!(error.starts_with("admission_unknown:"));
    assert!(fixture.service.chat_send_ledger.status(&refused_id).is_none());
    assert_eq!(fixture.service.chat_send_ledger.status(&accepted.send_id).unwrap().state, ChatSendState::Pending);
    let reopened = UserSendStore::open(fixture.temp.path()).unwrap();
    assert_eq!(reopened.records(&tab).unwrap().len(), 1);
    assert_eq!(reopened.original(&tab, &accepted.send_id).unwrap(), FIRST_PAYLOAD);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires the shared interactive desktop ServiceContext fixture")]
fn zero_write_owner_refusal_retains_head_and_disarms_turn_confirmation() {
    let (fixture, tab) = queue_fixture(false, None);
    let first = start_current_chat_send(&fixture, &tab, FIRST_PAYLOAD, "client-a");
    let second = start_current_chat_send(&fixture, &tab, SECOND_PAYLOAD, "client-b");
    let (written_tx, written_rx) = mpsc::channel();
    let _boundary = UserSubmitBoundary::install(&tab, Arc::new(move |record, _| {
        written_tx.send(record.send_id.clone()).unwrap();
        Err(DeliveryError::TabBusyRefused { tab: record.authority.tab_id.clone(), detail: "opaque-zero-write".into() })
    }));
    let (listener, settled) = settled_listener(&fixture, &tab);
    mark_ready(&fixture, &tab);
    fixture.service.wake_parked_user_send(&tab);
    assert_eq!(written_rx.recv_timeout(TEST_DEADLINE).unwrap(), first.send_id);
    assert_eq!(settled.recv_timeout(TEST_DEADLINE).unwrap()["state"], "unknown");
    fixture._app.unlisten(listener);
    let records = fixture.service.pending_delivery_payloads.user_sends.records(&tab).unwrap();
    assert_eq!(records[0].state, UserSendState::Queued);
    assert!(records[0].cycle_settled);
    assert_eq!(records[1].state, UserSendState::Queued);
    assert!(live_turn(&fixture, &tab, "opaque-session", "opaque-current-nonce", "unwritten-turn", FIRST_PAYLOAD).is_empty());
    assert!(!fixture.service.chat_send_ledger.user_confirmed(&first.send_id));
    assert_reopened_originals(&fixture, &[first, second]);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires the shared interactive desktop ServiceContext fixture")]
fn user_admissions_leave_existing_interactive_bootstrap_record_and_bytes_unchanged() {
    let (fixture, tab) = queue_fixture(true, None);
    let digest = fixture.service.create_pending_delivery(&tab, PendingDeliveryChannel::InteractiveInput, "opaque-bootstrap").unwrap();
    let before = fixture.service.mux.read().get_tab(&tab).unwrap().pending_delivery.clone().unwrap();
    assert_eq!(before.send_id(), None);
    let first = start_current_chat_send(&fixture, &tab, FIRST_PAYLOAD, "client-a");
    let second = start_current_chat_send(&fixture, &tab, SECOND_PAYLOAD, "client-b");
    fixture.service.drain_user_owner(&tab).unwrap();
    let after = fixture.service.mux.read().get_tab(&tab).unwrap().pending_delivery.clone().unwrap();
    assert_eq!(serde_json::to_value(&before).unwrap(), serde_json::to_value(&after).unwrap());
    assert_eq!(fixture.service.pending_delivery_payloads.resolve(&digest).unwrap(), "opaque-bootstrap");
    assert_reopened_originals(&fixture, &[first, second]);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires the shared interactive desktop ServiceContext fixture")]
fn ready_bootstrap_and_user_handoffs_share_one_coalesced_owner() {
    let (fixture, tab) = queue_fixture(true, None);
    let digest = fixture.service.create_pending_delivery(&tab, PendingDeliveryChannel::InteractiveInput, "opaque-bootstrap").unwrap();
    let first = start_current_chat_send(&fixture, &tab, FIRST_PAYLOAD, "client-a");
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let active = Arc::new(AtomicUsize::new(0));
    let bootstrap_active = active.clone();
    let _bootstrap = BootstrapSubmitBoundary::install(&tab, Arc::new(move |text| {
        assert_eq!(text, "opaque-bootstrap");
        assert_eq!(bootstrap_active.fetch_add(1, Ordering::AcqRel), 0);
        bootstrap_tx.send(()).unwrap();
        release_rx.lock().recv_timeout(TEST_DEADLINE).unwrap();
        assert_eq!(bootstrap_active.fetch_sub(1, Ordering::AcqRel), 1);
        Ok(())
    }));
    let (writes_tx, writes_rx) = mpsc::channel();
    let _user = UserSubmitBoundary::install(&tab, Arc::new(move |record, _| {
        assert_eq!(active.fetch_add(1, Ordering::AcqRel), 0);
        writes_tx.send(record.send_id.clone()).unwrap();
        assert_eq!(active.fetch_sub(1, Ordering::AcqRel), 1);
        Err(DeliveryError::SubmitUnconfirmed { tab: record.authority.tab_id.clone(), detail: "opaque-uncertainty".into() })
    }));
    fixture.service.wake_parked_user_send(&tab);
    assert!(matches!(bootstrap_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    assert!(fixture.service.mux.read().get_tab(&tab).unwrap().pending_delivery.is_some());
    mark_ready(&fixture, &tab);
    fixture.service.wake_parked_user_send(&tab);
    bootstrap_rx.recv_timeout(TEST_DEADLINE).unwrap();
    fixture.service.wake_parked_user_send(&tab);
    assert!(matches!(writes_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    assert!(fixture.service.mux.read().get_tab(&tab).unwrap().pending_delivery.is_none());
    release_tx.send(()).unwrap();
    assert_eq!(writes_rx.recv_timeout(TEST_DEADLINE).unwrap(), first.send_id);
    assert!(matches!(bootstrap_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    assert!(fixture.service.pending_delivery_payloads.resolve(&digest).is_err());
}

#[test]
#[cfg_attr(not(windows), ignore = "requires the shared interactive desktop ServiceContext fixture")]
fn held_user_store_does_not_suppress_existing_ready_bootstrap_consumer() {
    let (fixture, tab) = queue_fixture(true, None);
    let digest = fixture.service.create_pending_delivery(&tab, PendingDeliveryChannel::InteractiveInput, "opaque-bootstrap").unwrap();
    fixture.service.pending_delivery_payloads.user_sends.tab(&tab).failed.store(true, Ordering::Release);
    let writes = Arc::new(AtomicUsize::new(0));
    let count = writes.clone();
    let _bootstrap = BootstrapSubmitBoundary::install(&tab, Arc::new(move |text| {
        assert_eq!(text, "opaque-bootstrap");
        count.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }));
    let _user = UserSubmitBoundary::install(&tab, Arc::new(|_, _| panic!("held user owner must not submit")));
    mark_ready(&fixture, &tab);
    fixture.service.drain_user_owner(&tab).unwrap();
    assert_eq!(writes.load(Ordering::Acquire), 1);
    assert!(fixture.service.mux.read().get_tab(&tab).unwrap().pending_delivery.is_none());
    assert!(fixture.service.pending_delivery_payloads.resolve(&digest).is_err());
    assert!(fixture.service.pending_delivery_payloads.user_sends.records(&tab).unwrap().is_empty());
}

#[test]
#[cfg_attr(not(windows), ignore = "requires the shared interactive desktop ServiceContext fixture")]
fn legacy_user_slot_is_held_without_replay_adoption_or_payload_cleanup() {
    let (fixture, tab) = queue_fixture(true, None);
    let digest = fixture.service.create_pending_delivery(&tab, PendingDeliveryChannel::InteractiveInput, "opaque-legacy-original").unwrap();
    {
        let mut mux = fixture.service.mux.write();
        let pending = mux.get_tab_mut(&tab).unwrap().pending_delivery.take().unwrap().with_send_id("opaque-legacy-id".into());
        mux.get_tab_mut(&tab).unwrap().pending_delivery = Some(pending);
    }
    let before = fixture.service.mux.read().get_tab(&tab).unwrap().pending_delivery.clone().unwrap();
    let _bootstrap = BootstrapSubmitBoundary::install(&tab, Arc::new(|_| panic!("legacy user slot must remain held")));
    mark_ready(&fixture, &tab);
    fixture.service.drain_user_owner(&tab).unwrap();
    let after = fixture.service.mux.read().get_tab(&tab).unwrap().pending_delivery.clone().unwrap();
    assert_eq!(serde_json::to_value(&before).unwrap(), serde_json::to_value(&after).unwrap());
    assert_eq!(fixture.service.pending_delivery_payloads.resolve(&digest).unwrap(), "opaque-legacy-original");
    assert!(fixture.service.pending_delivery_payloads.user_sends.records(&tab).unwrap().is_empty());
}

#[test]
#[cfg_attr(not(windows), ignore = "requires the shared interactive desktop ServiceContext fixture")]
fn full_user_owner_refuses_new_admission_without_consuming_backend_ledger_capacity() {
    let (fixture, tab) = queue_fixture(true, None);
    let accepted = (0..crate::service::pending_delivery_payloads::user_sends::MAX_UNSETTLED)
        .map(|index| start_current_chat_send(&fixture, &tab, FIRST_PAYLOAD, &format!("opaque-client-{index}")))
        .collect::<Vec<_>>();
    let refusal = fixture.service.start_chat_send(&tab, SECOND_PAYLOAD.into(), None, Some("opaque-refused-client".into())).unwrap_err();
    assert!(refusal.contains("capacity"));
    assert!(fixture.service.chat_send_ledger.try_start_scoped_with_normalization(
        "opaque-backend-ledger", "opaque-backend", None, None, Default::default(),
    ).is_ok());
    let reopened = UserSendStore::open(fixture.temp.path()).unwrap();
    assert_eq!(reopened.records(&tab).unwrap().len(), accepted.len());
    for status in accepted {
        assert_eq!(reopened.original(&tab, &status.send_id).unwrap(), FIRST_PAYLOAD);
    }
}
