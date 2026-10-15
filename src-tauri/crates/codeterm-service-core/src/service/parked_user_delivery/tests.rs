pub(super) use super::test_support::*;
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
use std::time::Instant;

const FIRST_PAYLOAD: &str = " \topaque-a-41c9\r\n\u{03bb}\t ";
const SECOND_PAYLOAD: &str = "\topaque-b-8d27\n\u{754c}  ";
const REFUSAL: &str = "ProviderInputNotReady: opaque-readiness-boundary";

#[test]
fn first_turn_session_capability_allows_readiness_without_a_resume_id() {
    assert!(!requires_observed_session_id_for_readiness(None, true));
    assert!(requires_observed_session_id_for_readiness(None, false));
    assert!(!requires_observed_session_id_for_readiness(
        Some("observed-session"),
        false
    ));
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
    // The launch never reaches AgentReady here, so the refusal is the deadline itself.
    let _deadline = crate::service::pty_io::test_support::PublicInputReadyDeadline::set(
        std::time::Duration::ZERO,
    );
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
    let listener = fixture.ui.listen("chat-send-status", move |event| {
        let value: serde_json::Value = event.clone();
        if value["tab_id"] == expected_tab {
            let _ = tx.send(value);
        }
    });
    let accepted = fixture
        .service
        .start_chat_send(tab_id, payload.into(), None, Some(client_token.into()))
        .unwrap();
    let published = receive_send_status(&rx, &accepted.send_id);
    fixture.ui.unlisten(listener);
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
) -> (usize, mpsc::Receiver<serde_json::Value>) {
    let (tx, rx) = mpsc::channel();
    let tab = tab.to_string();
    let send_id = send_id.to_string();
    let listener = fixture.ui.listen("chat-send-status", move |event| {
        let value: serde_json::Value = event.clone();
        if value["tab_id"] == tab
            && value["send_id"] == send_id
            && value["state"].as_str() != Some("written")
        {
            let _ = tx.send(value);
        }
    });
    (listener, rx)
}
