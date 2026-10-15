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
                if take_parked_user_send(&mut pending.lock(), &record_id, Some("send-1")).is_some()
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
use std::time::{Duration, Instant};
use tauri::{Listener, Manager};

const FIRST_PAYLOAD: &str = " \topaque-a-41c9\r\n\u{03bb}\t ";
const SECOND_PAYLOAD: &str = "\topaque-b-8d27\n\u{754c}  ";
const REFUSAL: &str = "ProviderInputNotReady: opaque-readiness-boundary";
const TEST_DEADLINE: Duration = Duration::from_secs(10);

#[test]
fn first_turn_session_capability_allows_readiness_without_a_resume_id() {
    assert!(!requires_observed_session_id_for_readiness(None, true));
    assert!(requires_observed_session_id_for_readiness(None, false));
    assert!(!requires_observed_session_id_for_readiness(
        Some("observed-session"),
        false
    ));
}

pub(crate) fn pre_ready_fixture() -> (ServiceFixture, String) {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("opaque-provider".into());
    tab.pty_id = Some("opaque-pty".into());
    tab.agent_session_id = Some("opaque-session".into());
    tab.session_binding_nonce = Some("opaque-current-nonce".into());
    tab.reported_session_binding_nonce = Some("opaque-old-nonce".into());
    tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentLaunching);
    tab.boot_failure_reason = None;
    let fixture = ServiceFixture::new(mux);
    let session: crate::session::SessionState = Arc::new(Mutex::new(
        crate::session::SessionStore::for_test_dir(fixture.temp.path().join("sessions")),
    ));
    assert!(fixture._app.manage(session));
    assert!(fixture
        ._app
        .manage(Arc::new(fixture.service.clone()) as crate::service::ServiceState));
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
        assert!(
            matches!(&error, DeliveryError::TabBusyRefused { .. }),
            "error={error:?}"
        );
        assert!(!error.bytes_written());
        assert!(svc.pty.lock().get_session("opaque-pty").is_none());
        svc.park_refused_user_send(&tab_id, &send_id, text, &error);
        error.into_surfaced(SendOrigin::User)
    });
    assert_eq!(status.state, ChatSendState::Pending);
    assert_eq!(status.error, None);
}

fn assert_reopened_originals(fixture: &ServiceFixture, accepted: &[ChatSendStatus; 2]) {
    assert_ne!(accepted[0].send_id, accepted[1].send_id);
    assert!(accepted[0].sequence < accepted[1].sequence);
    let reopened = UserSendStore::open(fixture.temp.path()).unwrap();
    let records = reopened.records(&accepted[0].tab_id).unwrap();
    assert_eq!(
        records
            .iter()
            .map(|record| &record.send_id)
            .collect::<Vec<_>>(),
        vec![&accepted[0].send_id, &accepted[1].send_id]
    );
    assert!(records[0].acceptance_sequence < records[1].acceptance_sequence);
    let payloads = accepted
        .each_ref()
        .map(|status| reopened.original(&status.tab_id, &status.send_id));
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
    let authority = fixture.service.user_send_authority(tab_id).unwrap();
    let boot_stage = fixture
        .service
        .mux
        .read()
        .get_tab(tab_id)
        .unwrap()
        .boot_stage;
    let (tx, rx) = mpsc::channel();
    let expected_tab = tab_id.to_string();
    let listener = fixture._app.listen("chat-send-status", move |event| {
        let value: serde_json::Value = serde_json::from_str(event.payload()).unwrap();
        if value["tab_id"] == expected_tab {
            let _ = tx.send(value);
        }
    });
    let accepted = fixture
        .service
        .start_chat_send(tab_id, payload.into(), None, Some(client_token.into()))
        .unwrap();
    let published = receive_send_status(&rx, &accepted.send_id);
    fixture._app.unlisten(listener);
    let published = published.expect("the current service completed admission and parking");
    assert_eq!(published["send_id"], accepted.send_id);
    assert!(matches!(
        published["state"].as_str(),
        Some("held" | "submit_unconfirmed")
    ));
    assert!(published["error"].is_null());
    assert_eq!(accepted.correlation_id.as_deref(), Some(client_token));
    await_owner_idle(fixture, tab_id);
    assert_eq!(
        fixture.service.user_send_authority(tab_id).unwrap(),
        authority
    );
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(tab_id)
            .unwrap()
            .boot_stage,
        boot_stage
    );
    assert!(!fixture
        .service
        .user_authority_is_ready(&fixture.service.user_send_authority(tab_id).unwrap()));
    accepted
}

#[test]
fn pre_ready_two_accepted_user_sends_retain_originals_after_reopen() {
    let (fixture, tab_id) = pre_ready_fixture();
    let first = start_current_chat_send(&fixture, &tab_id, FIRST_PAYLOAD, "opaque-client-a");
    assert_eq!(
        fixture
            .service
            .pending_delivery_payloads
            .user_sends
            .original(&tab_id, &first.send_id)
            .unwrap(),
        FIRST_PAYLOAD,
        "first admission must actually reach the real payload store"
    );
    let second = start_current_chat_send(&fixture, &tab_id, SECOND_PAYLOAD, "opaque-client-b");

    assert_reopened_originals(&fixture, &[first, second]);
}

#[test]
fn reverse_transport_completion_retains_both_accepted_originals_after_reopen() {
    let (fixture, tab_id) = pre_ready_fixture();
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

pub(crate) type SubmitBoundary =
    Arc<dyn Fn(&UserSendRecord, &str) -> Result<(), DeliveryError> + Send + Sync>;
static SUBMIT_BOUNDARIES: std::sync::OnceLock<
    Mutex<std::collections::HashMap<String, SubmitBoundary>>,
> = std::sync::OnceLock::new();

pub struct UserSubmitBoundary(String);

impl UserSubmitBoundary {
    pub fn install(tab_id: &str, boundary: SubmitBoundary) -> Self {
        SUBMIT_BOUNDARIES
            .get_or_init(Default::default)
            .lock()
            .insert(tab_id.to_string(), boundary);
        Self(tab_id.to_string())
    }
}

impl Drop for UserSubmitBoundary {
    fn drop(&mut self) {
        SUBMIT_BOUNDARIES
            .get_or_init(Default::default)
            .lock()
            .remove(&self.0);
    }
}

pub(super) fn submit_boundary(tab_id: &str) -> Option<SubmitBoundary> {
    SUBMIT_BOUNDARIES
        .get_or_init(Default::default)
        .lock()
        .get(tab_id)
        .cloned()
}

fn receive_send_status(
    receiver: &mpsc::Receiver<serde_json::Value>,
    send_id: &str,
) -> Result<serde_json::Value, mpsc::RecvTimeoutError> {
    let deadline = Instant::now() + TEST_DEADLINE;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(mpsc::RecvTimeoutError::Timeout);
        }
        let status = receiver.recv_timeout(remaining)?;
        if status["send_id"].as_str() == Some(send_id) {
            return Ok(status);
        }
    }
}

fn receive_resolved_status(
    receiver: &mpsc::Receiver<serde_json::Value>,
) -> Result<serde_json::Value, mpsc::RecvTimeoutError> {
    let deadline = Instant::now() + TEST_DEADLINE;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(mpsc::RecvTimeoutError::Timeout);
        }
        let status = receiver.recv_timeout(remaining)?;
        if status["state"].as_str() != Some("held") {
            return Ok(status);
        }
    }
}

type BeforeSubmit = Arc<dyn Fn() + Send + Sync>;
static BEFORE_SUBMIT_BOUNDARIES: std::sync::OnceLock<
    Mutex<std::collections::HashMap<String, BeforeSubmit>>,
> = std::sync::OnceLock::new();

pub(crate) struct BeforeUserSubmitBoundary(String);

impl BeforeUserSubmitBoundary {
    pub fn install(tab_id: &str, before_submit: BeforeSubmit) -> Self {
        BEFORE_SUBMIT_BOUNDARIES
            .get_or_init(Default::default)
            .lock()
            .insert(tab_id.into(), before_submit);
        Self(tab_id.into())
    }
}

impl Drop for BeforeUserSubmitBoundary {
    fn drop(&mut self) {
        BEFORE_SUBMIT_BOUNDARIES
            .get_or_init(Default::default)
            .lock()
            .remove(&self.0);
    }
}

pub(super) fn run_before_submit(tab_id: &str) {
    if let Some(before_submit) = BEFORE_SUBMIT_BOUNDARIES
        .get_or_init(Default::default)
        .lock()
        .get(tab_id)
        .cloned()
    {
        before_submit();
    }
}

type BootstrapBoundary = Arc<dyn Fn(&str) -> Result<(), DeliveryError> + Send + Sync>;
static BOOTSTRAP_BOUNDARIES: std::sync::OnceLock<
    Mutex<std::collections::HashMap<String, BootstrapBoundary>>,
> = std::sync::OnceLock::new();

struct BootstrapSubmitBoundary(String);

impl BootstrapSubmitBoundary {
    fn install(tab_id: &str, boundary: BootstrapBoundary) -> Self {
        BOOTSTRAP_BOUNDARIES
            .get_or_init(Default::default)
            .lock()
            .insert(tab_id.to_string(), boundary);
        Self(tab_id.to_string())
    }
}

impl Drop for BootstrapSubmitBoundary {
    fn drop(&mut self) {
        BOOTSTRAP_BOUNDARIES
            .get_or_init(Default::default)
            .lock()
            .remove(&self.0);
    }
}

pub(super) fn bootstrap_boundary(tab_id: &str) -> Option<BootstrapBoundary> {
    BOOTSTRAP_BOUNDARIES
        .get_or_init(Default::default)
        .lock()
        .get(tab_id)
        .cloned()
}

type OwnerIdle = Arc<dyn Fn() + Send + Sync>;
static OWNER_IDLE_BOUNDARIES: std::sync::OnceLock<
    Mutex<std::collections::HashMap<String, OwnerIdle>>,
> = std::sync::OnceLock::new();

struct OwnerIdleBoundary(String);

impl OwnerIdleBoundary {
    fn install(tab_id: &str, boundary: OwnerIdle) -> Self {
        OWNER_IDLE_BOUNDARIES
            .get_or_init(Default::default)
            .lock()
            .insert(tab_id.to_string(), boundary);
        Self(tab_id.to_string())
    }
}

impl Drop for OwnerIdleBoundary {
    fn drop(&mut self) {
        OWNER_IDLE_BOUNDARIES
            .get_or_init(Default::default)
            .lock()
            .remove(&self.0);
    }
}

pub(super) fn notify_owner_idle(tab_id: &str) {
    let boundary = OWNER_IDLE_BOUNDARIES
        .get_or_init(Default::default)
        .lock()
        .get(tab_id)
        .cloned();
    if let Some(boundary) = boundary {
        boundary();
    }
}

struct QueueProvider {
    root: Option<std::path::PathBuf>,
    paste_limit: Option<usize>,
    lazy_session_start: bool,
}

impl crate::agent::provider::AgentProvider for QueueProvider {
    fn id(&self) -> &str {
        "opaque-provider"
    }
    fn display_name(&self) -> &str {
        "Opaque Provider"
    }
    fn build_launch_command(&self, _: &crate::agent::provider::AgentLaunchParams) -> String {
        String::new()
    }
    fn readiness_strategy(&self) -> crate::agent::provider::ReadinessStrategy {
        crate::agent::provider::ReadinessStrategy::FixedDelay {
            duration: Duration::ZERO,
        }
    }
    fn task_delivery(&self) -> crate::agent::provider::TaskDelivery {
        crate::agent::provider::TaskDelivery::PtyInput
    }
    fn session_starts_on_first_user_turn(&self) -> bool {
        self.lazy_session_start
    }
    fn detect_from_title(&self, _: &str) -> bool {
        false
    }
    fn detect_from_output(&self, _: &str) -> bool {
        false
    }
    fn session_file_path(&self, _: &str, session: &str) -> Option<std::path::PathBuf> {
        self.root
            .as_ref()
            .map(|root| root.join(format!("{session}.jsonl")))
    }
    fn session_jsonl_path(&self, cwd: &str, session: &str) -> Option<std::path::PathBuf> {
        self.session_file_path(cwd, session)
    }
    fn session_exists(&self, _: &str, _: &str) -> Option<bool> {
        Some(true)
    }
    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        None
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
    fn paste_limit_bytes(&self) -> Option<usize> {
        self.paste_limit
    }
    fn parse_session_delta(
        &self,
        path: &std::path::Path,
        from_offset: u64,
    ) -> crate::agent::chat_types::SessionDelta {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = std::fs::File::open(path).unwrap();
        file.seek(SeekFrom::Start(from_offset)).unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        let messages = bytes
            .split(|byte| *byte == b'\n')
            .filter_map(|line| {
                let value: serde_json::Value = serde_json::from_slice(line).ok()?;
                Some(crate::agent::chat_types::RichMessage {
                    role: "user".into(),
                    blocks: vec![crate::agent::chat_types::MessageBlock {
                        kind: "text".into(),
                        data: serde_json::json!({ "text": value["content"].as_str()? }),
                    }],
                    ..Default::default()
                })
            })
            .collect();
        crate::agent::chat_types::SessionDelta {
            new_offset: from_offset + bytes.len() as u64,
            messages,
        }
    }
}

pub fn queue_fixture(anchor: bool, paste_limit: Option<usize>) -> (ServiceFixture, String) {
    queue_fixture_with_session_mode(anchor, paste_limit, false)
}

fn queue_fixture_with_session_mode(
    anchor: bool,
    paste_limit: Option<usize>,
    lazy_session_start: bool,
) -> (ServiceFixture, String) {
    let (fixture, tab) = pre_ready_fixture();
    let root = fixture.temp.path().join("transcripts");
    std::fs::create_dir(&root).unwrap();
    if anchor {
        std::fs::write(root.join("opaque-session.jsonl"), b"").unwrap();
    }
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(QueueProvider {
            root: Some(root),
            paste_limit,
            lazy_session_start,
        }));
    (fixture, tab)
}

pub(crate) fn mark_ready(fixture: &ServiceFixture, tab_id: &str) {
    attach_fixture_pty(fixture, tab_id);
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(tab_id).unwrap();
        tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
        tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);
        tab.boot_failure_reason = None;
    }
    let authority = fixture.service.user_send_authority(tab_id).unwrap();
    assert!(fixture.service.latch_user_authority_ready(&authority));
}

#[derive(Clone)]
struct ObservedTurn {
    message: crate::agent::chat_types::ChatMsg,
    provenance: crate::service::chat_send_transaction::LiveProvenance,
}

fn observed_turn(fixture: &ServiceFixture, tab: &str, id: &str, text: &str) -> ObservedTurn {
    let path = fixture
        .temp
        .path()
        .join("transcripts")
        .join("opaque-session.jsonl");
    let bytes = std::fs::read(&path).unwrap();
    let content_end = bytes
        .iter()
        .rposition(|byte| *byte != b'\n')
        .map_or(0, |index| index + 1);
    let record_start = bytes[..content_end]
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1) as u64;
    let source = fixture.service.chat_registry.jsonl_source(tab);
    let crate::service::tab_chat::registry::SourceObservation::Present {
        incarnation: Some(incarnation),
        ..
    } = source.observe("opaque-session", &path, None, record_start)
    else {
        panic!("the transcript must have a sourced incarnation");
    };
    let nonce = fixture
        .service
        .mux
        .read()
        .get_tab(tab)
        .and_then(|tab| tab.session_binding_nonce.clone())
        .unwrap();
    ObservedTurn {
        message: crate::agent::chat_types::ChatMsg {
            id: id.into(),
            kind: "user".into(),
            content: text.into(),
            ..Default::default()
        },
        provenance: crate::service::chat_send_transaction::LiveProvenance {
            source_session_id: "opaque-session".into(),
            source_binding_nonce: Some(nonce),
            range: crate::service::tab_chat::tailer::SourceRange {
                start_offset: record_start,
                offset: bytes.len() as u64,
                incarnation: Some(incarnation),
            },
            positions: vec![crate::service::chat_send_transaction::TurnPosition {
                record_start: Some(record_start),
                record_row: 0,
            }],
        },
    }
}

fn deliver_turn(
    fixture: &ServiceFixture,
    tab: &str,
    session: &str,
    nonce: &str,
    turn: &ObservedTurn,
) -> Vec<ChatSendStatus> {
    crate::service::tab_chat::structured_sink::current_binding_turn_confirmations(
        &fixture.service.mux,
        Some(&fixture.service.chat_send_ledger),
        tab,
        Some(session),
        Some(nonce),
        &[turn.message.clone()],
        Some(&turn.provenance),
    )
}

pub(crate) fn live_turn(
    fixture: &ServiceFixture,
    tab: &str,
    session: &str,
    nonce: &str,
    id: &str,
    text: &str,
) -> Vec<ChatSendStatus> {
    let path = fixture
        .temp
        .path()
        .join("transcripts")
        .join("opaque-session.jsonl");
    if !path.exists() {
        return Vec::new();
    }
    let turn = observed_turn(fixture, tab, id, text);
    deliver_turn(fixture, tab, session, nonce, &turn)
}

fn append_turn(fixture: &ServiceFixture, text: &str) {
    use std::io::Write;
    let path = fixture
        .temp
        .path()
        .join("transcripts")
        .join("opaque-session.jsonl");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    writeln!(file, "{}", serde_json::json!({ "content": text })).unwrap();
}

fn owner_state(fixture: &ServiceFixture, tab: &str) -> String {
    let store = &fixture.service.pending_delivery_payloads.user_sends;
    let owner = store.tab(tab);
    let records = store.records(tab).map(|records| {
        records
            .iter()
            .map(|record| record.state)
            .collect::<Vec<_>>()
    });
    format!(
        "failed={} revoked={} running={} records={records:?}",
        owner.failed.load(Ordering::Acquire),
        owner.revoked.load(Ordering::Acquire),
        owner.running.load(Ordering::Acquire)
    )
}

fn settled_listener(
    fixture: &ServiceFixture,
    tab: &str,
    send_id: &str,
) -> (tauri::EventId, mpsc::Receiver<serde_json::Value>) {
    let (tx, rx) = mpsc::channel();
    let tab = tab.to_string();
    let send_id = send_id.to_string();
    let listener = fixture._app.listen("chat-send-status", move |event| {
        let value: serde_json::Value = serde_json::from_str(event.payload()).unwrap();
        if value["tab_id"] == tab
            && value["send_id"] == send_id
            && value["state"].as_str() != Some("written")
        {
            let _ = tx.send(value);
        }
    });
    (listener, rx)
}

pub fn await_owner_idle(fixture: &ServiceFixture, tab_id: &str) {
    fixture
        .service
        .ready_user_send_tasks
        .wait(tab_id, TEST_DEADLINE);
    let owner = fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .tab(tab_id);
    let (idle_tx, idle_rx) = mpsc::sync_channel(1);
    let callback_tx = idle_tx.clone();
    let _boundary = OwnerIdleBoundary::install(
        tab_id,
        Arc::new(move || {
            let _ = callback_tx.try_send(());
        }),
    );
    fixture
        .service
        .wake_user_owner(tab_id, super::UserWake::Housekeeping);
    loop {
        idle_rx
            .recv_timeout(TEST_DEADLINE)
            .expect("user owner reaches its idle boundary");
        if !owner.running.load(Ordering::Acquire) {
            fixture
                .service
                .ready_user_send_tasks
                .wait(tab_id, TEST_DEADLINE);
            return;
        }
    }
}

pub(crate) fn await_owner_stopped(fixture: &ServiceFixture, tab_id: &str) {
    let owner = fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .tab(tab_id);
    let (idle_tx, idle_rx) = mpsc::sync_channel(1);
    let callback_tx = idle_tx.clone();
    let _boundary = OwnerIdleBoundary::install(
        tab_id,
        Arc::new(move || {
            let _ = callback_tx.try_send(());
        }),
    );
    loop {
        if !owner.running.load(Ordering::Acquire) {
            return;
        }
        idle_rx
            .recv_timeout(TEST_DEADLINE)
            .expect("user owner stops after its held send");
    }
}

pub fn attach_fixture_pty(fixture: &ServiceFixture, tab_id: &str) -> u64 {
    let pty_id = fixture
        .service
        .mux
        .read()
        .get_tab(tab_id)
        .unwrap()
        .pty_id
        .clone()
        .unwrap();
    if let Some(session) = fixture.service.pty.lock().get_session(&pty_id) {
        return session.generation;
    }
    let (_, session) = fixture
        .service
        .pty
        .lock()
        .spawn_with_id(
            Some(pty_id.clone()),
            None,
            None,
            120,
            40,
            Default::default(),
        )
        .expect("spawn the isolated ServiceContext PTY fixture");
    let generation = session.generation;
    assert!(fixture
        .service
        .screen_state
        .lock()
        .bind_pty_generation(&pty_id, generation));
    generation
}

#[path = "tests/bootstrap_interaction.rs"]
mod bootstrap_interaction;
#[path = "tests/capacity.rs"]
mod capacity;
#[path = "tests/durable_admission.rs"]
mod durable_admission;
#[path = "tests/ended_launch.rs"]
mod ended_launch;
mod evidence;
#[path = "tests/lazy_session.rs"]
mod lazy_session;
#[path = "tests/merged_turns.rs"]
mod merged_turns;
#[path = "tests/owner_jev.rs"]
mod owner_jev;
#[path = "tests/readiness.rs"]
mod readiness;
mod recording_contract;
#[path = "tests/retained_uncertainty.rs"]
mod retained_uncertainty;

mod ready_order;

pub(super) mod queued_boundary;
