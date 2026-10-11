use super::*;
use crate::service::local_recording::leases::{Capture, CapturePhase, FrozenTarget};
use codeterm_plugin::local_recording::{LocalRecordingTrigger, SelectedDevices};
use std::sync::atomic::AtomicBool;

fn capture(target: FrozenTarget) -> Capture {
    Capture {
        lease_id: "fixture-lease".into(),
        plugin_id: "trigger".into(),
        target,
        active: Arc::new(AtomicBool::new(true)),
        expires_at: Arc::new(parking_lot::Mutex::new(Instant::now() + TEST_DEADLINE)),
        phase: CapturePhase::Transcribing,
    }
}

fn armed_capture(fixture: &ServiceFixture, tab: &str) -> (Capture, u64) {
    fixture.service.local_recording_registry.register(
        "trigger",
        LocalRecordingTrigger {
            vendor_id: 1,
            product_id: 2,
            usage_page: 3,
            usage: 4,
        },
        true,
    );
    let devices = SelectedDevices {
        input_id: "input".into(),
        collection_id: "collection".into(),
        microphone_id: "microphone".into(),
    };
    fixture
        .service
        .local_recording_registry
        .select_devices("trigger", devices.clone())
        .unwrap();
    fixture
        .service
        .local_recording_registry
        .set_enabled("trigger", true)
        .unwrap();
    let generation = fixture
        .service
        .local_recording_registry
        .selected_devices("trigger")
        .unwrap()
        .1;
    assert_eq!(
        crate::service::local_recording::arm_registry_fixture(
            &fixture.service.local_recording_registry,
            "trigger"
        ),
        generation
    );
    let target = fixture.service.freeze_recording_target(tab).unwrap();
    let lease = fixture
        .service
        .local_recordings
        .start(
            "trigger",
            &devices,
            generation,
            target,
            &fixture.service.local_recording_registry,
        )
        .unwrap();
    fixture
        .service
        .local_recordings
        .stopped(&lease.lease)
        .unwrap();
    let capture = fixture
        .service
        .local_recordings
        .transcribing(&lease.lease)
        .unwrap();
    (capture, generation)
}

enum Invalidation {
    Cancel,
    Revoke,
    Unload,
    DeviceLoss,
    Deadline,
    Retire,
}

fn assert_invalidated_before_write(invalidation: Invalidation) {
    let (fixture, tab) = queue_fixture(true, None);
    mark_ready(&fixture, &tab);
    let (capture, generation) = armed_capture(&fixture, &tab);
    let writes = Arc::new(AtomicUsize::new(0));
    let count = writes.clone();
    let _write = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |_, _| {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
    );
    let (at_guard_tx, at_guard_rx) = mpsc::channel();
    let (continue_tx, continue_rx) = mpsc::channel();
    let _barrier = BeforeUserSubmitBoundary::install(
        &tab,
        Arc::new(move || {
            at_guard_tx.send(()).unwrap();
            continue_rx.recv_timeout(TEST_DEADLINE).unwrap();
        }),
    );
    let accepted = fixture
        .service
        .admit_recording_voice(
            capture.clone(),
            codeterm_types::message_markers::voice("audio", Some("speech"), None),
            "capture-client".into(),
        )
        .unwrap();
    at_guard_rx.recv_timeout(TEST_DEADLINE).unwrap();
    assert!(
        fixture
            .service
            .local_recordings
            .has_write_ticket(&accepted.status.send_id)
    );
    match invalidation {
        Invalidation::Cancel => fixture.service.local_recordings.cancel(&capture.lease_id),
        Invalidation::Revoke => fixture.service.local_recording_registry.revoke("trigger"),
        Invalidation::Unload => fixture
            .service
            .local_recording_registry
            .unregister("trigger"),
        Invalidation::DeviceLoss => assert!(
            fixture
                .service
                .local_recording_registry
                .mark_unavailable("trigger", generation)
        ),
        Invalidation::Deadline => fixture
            .service
            .local_recordings
            .expire_write_ticket(&accepted.status.send_id),
        Invalidation::Retire => fixture
            .service
            .local_recordings
            .retire_write_ticket(&accepted.status.send_id),
    }
    continue_tx.send(()).unwrap();
    await_owner_idle(&fixture, &tab);
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    assert!(
        !fixture
            .service
            .local_recordings
            .has_write_ticket(&accepted.status.send_id)
    );
    assert!(
        fixture
            .service
            .pending_delivery_payloads
            .user_sends
            .queued(&tab)
            .unwrap()
            .is_empty()
    );
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn lark_recording_contract_identity_only_baseline_allows_cancelled_capture() {
    let (fixture, tab) = queue_fixture(true, None);
    mark_ready(&fixture, &tab);
    let (capture, _) = armed_capture(&fixture, &tab);
    let writes = Arc::new(AtomicUsize::new(0));
    let count = writes.clone();
    let _write = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |_, _| {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
    );
    let (at_guard_tx, at_guard_rx) = mpsc::channel();
    let (continue_tx, continue_rx) = mpsc::channel();
    let cancelled = capture.active.clone();
    let _barrier = BeforeUserSubmitBoundary::install(
        &tab,
        Arc::new(move || {
            at_guard_tx.send(()).unwrap();
            continue_rx.recv_timeout(TEST_DEADLINE).unwrap();
        }),
    );
    let reservation = fixture
        .service
        .reserve_user_send(&tab, Some("manual-client".into()))
        .unwrap();
    fixture
        .service
        .prepare_and_admit_user_send(reservation, "ordinary text".into(), false)
        .unwrap();
    at_guard_rx.recv_timeout(TEST_DEADLINE).unwrap();
    cancelled.store(false, Ordering::Release);
    continue_tx.send(()).unwrap();
    await_owner_idle(&fixture, &tab);
    assert_eq!(writes.load(Ordering::SeqCst), 1);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn lark_recording_contract_capture_cancel_revoke_unload_device_loss_and_deadline_deny_final_guard()
{
    assert_invalidated_before_write(Invalidation::Cancel);
    assert_invalidated_before_write(Invalidation::Revoke);
    assert_invalidated_before_write(Invalidation::Unload);
    assert_invalidated_before_write(Invalidation::DeviceLoss);
    assert_invalidated_before_write(Invalidation::Deadline);
    assert_invalidated_before_write(Invalidation::Retire);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn lark_recording_contract_registered_ticket_survives_admission_and_valid_write_retires_once() {
    let (fixture, tab) = queue_fixture(true, None);
    mark_ready(&fixture, &tab);
    let (capture, _) = armed_capture(&fixture, &tab);
    let writes = Arc::new(AtomicUsize::new(0));
    let count = writes.clone();
    let _write = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |_, _| {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
    );
    let (at_guard_tx, at_guard_rx) = mpsc::channel();
    let (continue_tx, continue_rx) = mpsc::channel();
    let _barrier = BeforeUserSubmitBoundary::install(
        &tab,
        Arc::new(move || {
            at_guard_tx.send(()).unwrap();
            continue_rx.recv_timeout(TEST_DEADLINE).unwrap();
        }),
    );
    let accepted = fixture
        .service
        .admit_recording_voice(
            capture,
            codeterm_types::message_markers::voice("audio", Some("speech"), None),
            "capture-client".into(),
        )
        .unwrap();
    at_guard_rx.recv_timeout(TEST_DEADLINE).unwrap();
    assert!(
        fixture
            .service
            .local_recordings
            .has_write_ticket(&accepted.status.send_id)
    );
    continue_tx.send(()).unwrap();
    await_owner_idle(&fixture, &tab);
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    assert!(
        !fixture
            .service
            .local_recordings
            .has_write_ticket(&accepted.status.send_id)
    );
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn lark_recording_contract_possible_handoff_remains_uncertain_and_never_replays() {
    let (fixture, tab) = queue_fixture(true, None);
    mark_ready(&fixture, &tab);
    let (capture, _) = armed_capture(&fixture, &tab);
    let writes = Arc::new(AtomicUsize::new(0));
    let count = writes.clone();
    let _write = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |_, _| {
            count.fetch_add(1, Ordering::SeqCst);
            Err(DeliveryError::SubmitUnconfirmed {
                tab: "recording-tab".into(),
                detail: "fixture accepted bytes before losing acknowledgment".into(),
            })
        }),
    );
    let accepted = fixture
        .service
        .admit_recording_voice(
            capture,
            codeterm_types::message_markers::voice("audio", Some("speech"), None),
            "capture-client".into(),
        )
        .unwrap();
    await_owner_idle(&fixture, &tab);
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&accepted.status.send_id)
            .unwrap()
            .state,
        ChatSendState::Unknown
    );
    assert!(
        fixture
            .service
            .pending_delivery_payloads
            .user_sends
            .queued(&tab)
            .unwrap()
            .is_empty()
    );
    assert!(
        !fixture
            .service
            .local_recordings
            .has_write_ticket(&accepted.status.send_id)
    );
    fixture.service.drain_user_owner(&tab).unwrap();
    assert_eq!(writes.load(Ordering::SeqCst), 1);
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
