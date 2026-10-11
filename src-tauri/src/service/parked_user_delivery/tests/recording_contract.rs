use super::*;
use crate::service::local_recording::leases::{Capture, CapturePhase, FrozenTarget};
use std::sync::atomic::AtomicBool;

fn capture(target: FrozenTarget) -> Capture {
    Capture {
        plugin_id: "trigger".into(),
        target,
        active: Arc::new(AtomicBool::new(true)),
        expires_at: Instant::now() + TEST_DEADLINE,
        phase: CapturePhase::Transcribing,
    }
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn lark_recording_contract_pty_freeze_reservation_rebind_is_zero_admission() {
    let (fixture, tab) = queue_fixture(true, None);
    mark_ready(&fixture, &tab);
    let frozen = fixture.service.freeze_recording_target(&tab).unwrap();
    fixture.service.rotate_session_binding_nonce(&tab);
    let result = fixture.service.admit_recording_voice(
        capture(frozen),
        "voice-token".into(),
        "recording-client".into(),
    );
    assert!(result.is_err());
    assert!(fixture.service.chat_send_ledger.reconcile(&tab).is_empty());
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn lark_recording_contract_pty_after_admission_before_write_reuses_existing_guard() {
    let (fixture, tab) = queue_fixture(true, None);
    mark_ready(&fixture, &tab);
    let frozen = fixture.service.freeze_recording_target(&tab).unwrap();
    let writes = Arc::new(AtomicUsize::new(0));
    let count = writes.clone();
    let _write = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |_, _| {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
    );
    let service = fixture.service.clone();
    let rebind_tab = tab.clone();
    let (guard_tx, guard_rx) = mpsc::channel();
    let _rebind = BeforeUserSubmitBoundary::install(
        &tab,
        Arc::new(move || {
            service.rotate_session_binding_nonce(&rebind_tab);
            guard_tx.send(()).unwrap();
        }),
    );
    let accepted = fixture
        .service
        .admit_recording_voice(
            capture(frozen),
            "voice-token".into(),
            "recording-client".into(),
        )
        .unwrap();
    guard_rx.recv_timeout(TEST_DEADLINE).unwrap();
    await_owner_idle(&fixture, &tab);
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    assert_eq!(
        accepted.status.correlation_id.as_deref(),
        Some("recording-client")
    );
    fixture.service.drain_user_owner(&tab).unwrap();
    assert_eq!(writes.load(Ordering::SeqCst), 0);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn lark_recording_contract_pty_valid_ordinary_tracked_voice_once() {
    let (fixture, tab) = queue_fixture(true, None);
    mark_ready(&fixture, &tab);
    let frozen = fixture.service.freeze_recording_target(&tab).unwrap();
    let (write_tx, write_rx) = mpsc::channel();
    let _write = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |record, text| {
            write_tx
                .send((record.authority.clone(), text.to_string()))
                .unwrap();
            Ok(())
        }),
    );
    let marker = codeterm_types::message_markers::voice("owned-audio", Some("speech-token"), None);
    let accepted = fixture
        .service
        .admit_recording_voice(
            capture(frozen.clone()),
            marker.clone(),
            "recording-client".into(),
        )
        .unwrap();
    let (authority, payload) = write_rx.recv_timeout(TEST_DEADLINE).unwrap();
    assert_eq!(FrozenTarget::Pty(authority), frozen);
    assert_eq!(payload, format!("\n\n{marker}"));
    await_owner_idle(&fixture, &tab);
    assert!(write_rx.try_recv().is_err());
    assert_eq!(
        accepted.status.correlation_id.as_deref(),
        Some("recording-client")
    );
}
