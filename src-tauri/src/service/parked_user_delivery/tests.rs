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
use crate::service::pending_delivery_payloads::{live_payload_digests, PendingDeliveryPayloadStore};
use crate::service::test_fixture::ServiceFixture;
use sha2::{Digest, Sha256};
use std::sync::mpsc;
use std::time::Duration;
use tauri::{Listener, Manager};

const FIRST_PAYLOAD: &str = " \topaque-a-41c9\r\n\u{03bb}\t ";
const SECOND_PAYLOAD: &str = "\topaque-b-8d27\n\u{754c}  ";
const REFUSAL: &str = "ProviderInputNotReady: opaque-readiness-boundary";
const TEST_DEADLINE: Duration = Duration::from_secs(10);

fn failed_launch_fixture() -> (ServiceFixture, String) {
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
    let session = crate::session::SessionStore::for_test_dir(fixture.temp.path().join("sessions"));
    let saved = session.load_last().unwrap().expect("parking persisted its metadata");
    let restored = crate::multiplexer::state::MultiplexerState::from_session_snapshot(&saved);
    let live = live_payload_digests(&restored.snapshot());
    let reopened = PendingDeliveryPayloadStore::open(fixture.temp.path(), &live).unwrap();
    let payloads = [FIRST_PAYLOAD, SECOND_PAYLOAD].map(|payload| {
        let digest = format!("{:x}", Sha256::digest(payload.as_bytes()));
        reopened.resolve(&digest)
    });
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
    accepted
}

#[test]
#[cfg_attr(not(windows), ignore = "requires the shared interactive desktop ServiceContext fixture")]
fn spawn_failed_two_accepted_user_sends_retain_originals_after_reopen() {
    let (fixture, tab_id) = failed_launch_fixture();
    let first = start_current_chat_send(&fixture, &tab_id, FIRST_PAYLOAD, "opaque-client-a");
    let first_digest = format!("{:x}", Sha256::digest(FIRST_PAYLOAD.as_bytes()));
    assert_eq!(
        fixture.service.pending_delivery_payloads.resolve(&first_digest).unwrap(),
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
