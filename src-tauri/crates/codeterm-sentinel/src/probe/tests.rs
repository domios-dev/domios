use super::capture::{self, CaptureData, CompletionFacts};
use super::owner::{ProbeResourceOwner, RetainedResources, Settlement};
use super::process::{cancellation_target, Deadlines, LeaderOwnership, WindowsLaunchState};
use super::PROBE_RUNTIME;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

const OWNER_EVENT_DEADLINE: Duration = Duration::from_secs(3);
const PROBE_COMPLETION_SCHEDULING_MARGIN: Duration = Duration::from_secs(3);

#[test]
fn sentinel_probe_bounded_runtime_and_output() {
    let dir = tempfile::tempdir().unwrap();
    let owner = Arc::new(ProbeResourceOwner::new());
    #[cfg(unix)]
    let stalled = "exec cat /dev/zero";
    #[cfg(windows)]
    let stalled = "while ($true) { [Console]::Out.Write('x') }";
    let run_ready = |command: Option<&str>, limit| {
        let ready = super::test_readiness::Readiness::new();
        let (signal, receipts) = ready.signal(limit, PROBE_RUNTIME);
        let command = command.map_or_else(
            || ready.output_command(),
            |command| ready.command_after_ready(command),
        );
        let cwd = dir.path().to_path_buf();
        let owner = owner.clone();
        let (completed_tx, completed_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result = super::run_cancellable(&command, &cwd, limit, &owner, &signal);
            completed_tx.send((result, Instant::now())).unwrap();
        });
        let receipt = receipts
            .recv_timeout(
                super::test_readiness::SETUP_DEADLINE + PROBE_COMPLETION_SCHEDULING_MARGIN,
            )
            .expect("probe child readiness receipt before setup deadline");
        assert_eq!(receipt.duration, limit.min(PROBE_RUNTIME));
        let completion_deadline = receipt.hard + PROBE_COMPLETION_SCHEDULING_MARGIN;
        let (result, completed_at) = completed_rx
            .recv_timeout(completion_deadline.saturating_duration_since(Instant::now()))
            .expect("probe completion before readiness-anchored deadline");
        assert!(
            completed_at <= completion_deadline,
            "probe completed {:?} after readiness with runtime {:?} and scheduling margin {:?}",
            completed_at.duration_since(receipt.admitted),
            receipt.duration,
            PROBE_COMPLETION_SCHEDULING_MARGIN,
        );
        worker.join().unwrap();
        (result, receipt)
    };
    let (trivial, receipt) = run_ready(Some("echo ok"), PROBE_RUNTIME);
    let trivial = trivial.unwrap_or_else(|error| {
        panic!(
            "trivial probe failed after readiness with runtime {:?}: {error}",
            receipt.duration,
        )
    });
    assert_eq!(
        i32::from_le_bytes(trivial.bytes[..4].try_into().unwrap()),
        0
    );
    let normalized = trivial.text.replace("\r\n", "\n");
    assert!(
        normalized.contains("stdout:\nok\n"),
        "unexpected formatted probe output: {:?}",
        trivial.text
    );
    let (output, receipt) = run_ready(None, PROBE_RUNTIME);
    let output = output.unwrap_or_else(|error| {
        panic!(
            "large probe failed after readiness with runtime {:?}: {error}",
            receipt.duration,
        )
    });
    assert!(output.bytes.len() <= super::MAX_OUTPUT_BYTES + 12);
    assert!(output.bytes.len() >= super::MAX_OUTPUT_BYTES / 2);
    assert!(output.stdout.truncated);
    assert!(output.stdout.total_bytes >= 1_000_000);
    assert_eq!(output.exit_code, Some(0));
    let (result, _) = run_ready(Some(stalled), Duration::from_millis(200));
    assert_eq!(result.unwrap_err().primary, "probe_timeout");
}

#[cfg(windows)]
#[test]
fn sentinel_probe_capture_windows_iocp_timeout_preserves_completion_ownership() {
    super::windows::ordering_tests::verify_empty_completion_port_timeout();
}

#[cfg(windows)]
#[test]
fn sentinel_probe_early_job_exit_waits_for_owned_process() {
    super::windows::ordering_tests::verify_early_exit_notification();
}

#[test]
fn sentinel_probe_owner_retirement_refuses_signal() {
    let child_id = 41;
    let mut calls = Vec::new();
    for ownership in [
        LeaderOwnership::Retired(child_id),
        LeaderOwnership::Lost,
        LeaderOwnership::Retained(0),
        LeaderOwnership::Retained(1),
        LeaderOwnership::Retained(child_id + 1),
    ] {
        if let Ok(group) = cancellation_target(ownership, child_id) {
            calls.push(group);
        }
    }
    assert!(calls.is_empty());

    let group = cancellation_target(LeaderOwnership::Retained(child_id), child_id).unwrap();
    calls.push(group);
    assert_eq!(calls, [child_id as i32]);
}

#[test]
fn sentinel_probe_windows_assigns_before_resume() {
    let mut state = WindowsLaunchState::default();
    assert!(state.resume().is_err());

    state.job_ready();
    assert!(state.resume().is_err());
    state.capture_ready();
    assert!(state.resume().is_err());
    state.process_captured().unwrap();
    assert!(state.resume().is_err());
    state.assigned().unwrap();
    state.resume().unwrap();
    assert!(state.resumed());
    assert!(state.resume().is_err());
}

#[test]
fn sentinel_probe_capture_failure_cannot_return_output() {
    let mut bounded = CaptureData::default();
    bounded.append(&vec![b'x'; 32 * 1024]);
    bounded.append(b"extra");
    assert_eq!(bounded.prefix.len(), 32 * 1024);
    assert_eq!(bounded.total_bytes, (32 * 1024 + 5) as u64);
    assert!(bounded.truncated);
    assert!(!bounded.eof);
    bounded.mark_eof();
    assert!(bounded.eof);

    let mut stdout = CaptureData::default();
    let mut stderr = CaptureData::default();
    stdout.append(b"partial stdout");
    stderr.append(b"partial stderr");
    stdout.mark_eof();
    stderr.mark_eof();

    for reason in [
        "read failed",
        "partial frame",
        "cancel completion unknown",
        "close failed",
        "completion packet unknown",
    ] {
        let result = capture::assemble_confirmed(
            test_status(),
            CaptureData {
                prefix: stdout.prefix.clone(),
                total_bytes: stdout.total_bytes,
                truncated: stdout.truncated,
                eof: stdout.eof,
            },
            CaptureData {
                prefix: stderr.prefix.clone(),
                total_bytes: stderr.total_bytes,
                truncated: stderr.truncated,
                eof: stderr.eof,
            },
            CompletionFacts {
                parent_complete: true,
                boundary_complete: true,
                stdout_eof: true,
                stderr_eof: true,
                error: Some(reason),
            },
        );
        let failure = result.expect_err("capture errors cannot create machine input");
        assert_eq!(failure.primary, "probe_capture_or_completion_failed");
        assert!(failure.cleanup_errors.iter().any(|item| item == reason));
    }

    let mut partial = CaptureData::default();
    partial.append(b"read bytes before failure");
    let failure = capture::assemble_confirmed(
        test_status(),
        partial,
        stderr,
        CompletionFacts {
            parent_complete: true,
            boundary_complete: true,
            stdout_eof: false,
            stderr_eof: true,
            error: None,
        },
    )
    .expect_err("missing EOF cannot be invented");
    assert_eq!(failure.primary, "probe_capture_eof_unconfirmed");
    assert!(failure.is_unconfirmed());

    for facts in [
        CompletionFacts {
            parent_complete: false,
            boundary_complete: true,
            stdout_eof: true,
            stderr_eof: true,
            error: None,
        },
        CompletionFacts {
            parent_complete: true,
            boundary_complete: false,
            stdout_eof: true,
            stderr_eof: true,
            error: None,
        },
    ] {
        assert!(capture::assemble_confirmed(
            test_status(),
            completed_data(b"out"),
            completed_data(b"err"),
            facts,
        )
        .is_err());
    }
}

#[test]
fn sentinel_probe_capture_error_keeps_process_completion_separate() {
    for (parent, boundary) in [(true, true), (false, true), (true, false)] {
        let failure = capture::assemble_confirmed(
            test_status(),
            completed_data(b"out"),
            completed_data(b"err"),
            CompletionFacts {
                parent_complete: parent,
                boundary_complete: boundary,
                stdout_eof: true,
                stderr_eof: true,
                error: Some("capture error"),
            },
        )
        .unwrap_err();
        assert_eq!(failure.processes_confirmed, parent && boundary);
        assert!(failure.is_unconfirmed());
        assert_eq!(
            failure
                .unconfirmed
                .contains(&super::process::ResourceKind::Child),
            !parent
        );
        assert_eq!(
            failure
                .unconfirmed
                .contains(&super::process::ResourceKind::ProcessGroup),
            !boundary
        );
    }
}

#[test]
fn sentinel_probe_original_deadline_is_not_renewed() {
    let admitted = Instant::now();
    let deadlines =
        Deadlines::at(Duration::from_secs(9), Duration::from_secs(3), admitted).unwrap();
    assert_eq!(deadlines.admitted, admitted);
    assert_eq!(deadlines.duration, Duration::from_secs(3));
    assert_eq!(deadlines.operation, admitted + Duration::from_millis(2500));
    assert_eq!(deadlines.hard, admitted + Duration::from_secs(3));

    for elapsed in [
        Duration::from_millis(100),
        Duration::from_secs(1),
        Duration::from_millis(2400),
    ] {
        let now = admitted + elapsed;
        assert_eq!(
            deadlines.remaining_operation_at(now),
            Duration::from_millis(2500).saturating_sub(elapsed)
        );
        assert_eq!(
            deadlines.remaining_hard_at(now),
            Duration::from_secs(3).saturating_sub(elapsed)
        );
        assert!(!deadlines.expired_operation_at(now));
    }
    let operation_end = admitted + Duration::from_millis(2500);
    assert!(deadlines.expired_operation_at(operation_end));
    assert_eq!(
        deadlines.remaining_hard_at(operation_end),
        Duration::from_millis(500)
    );
    assert_eq!(deadlines.hard, admitted + Duration::from_secs(3));

    let short =
        Deadlines::at(Duration::from_millis(100), Duration::from_secs(3), admitted).unwrap();
    assert_eq!(short.operation, admitted + Duration::from_millis(75));
    assert_eq!(short.hard, admitted + Duration::from_millis(100));
}

#[test]
fn sentinel_probe_owner_capacity_refuses_before_spawn() {
    let owner = Arc::new(ProbeResourceOwner::new());
    let reservations = (0..super::owner::MAX_RESOURCE_SLOTS)
        .map(|_| owner.reserve().unwrap())
        .collect::<Vec<_>>();
    let mut transaction_started = false;
    let failure = super::admit_then(&owner, |_| {
        transaction_started = true;
        Ok(())
    })
    .err()
    .unwrap();
    assert_eq!(failure.primary, "probe_owner_capacity");
    assert!(!transaction_started);
    for reservation in reservations {
        reservation.release().unwrap();
    }
}

#[test]
fn sentinel_probe_owner_close_refuses_and_requests_cleanup() {
    let owner = Arc::new(ProbeResourceOwner::new());
    let mut reservation = owner.reserve().unwrap();
    reservation.register_resource().unwrap();
    owner.close_admission();
    assert!(matches!(
        owner.reserve(),
        Err(super::owner::AdmissionError::Closed)
    ));

    let (cancel_tx, cancel_rx) = mpsc::channel();
    let (settle_tx, settle_rx) = mpsc::channel();
    let (dropped_tx, dropped_rx) = mpsc::channel();
    reservation.retain(Box::new(event_resource(
        1, cancel_tx, settle_rx, dropped_tx, None,
    )));
    assert_eq!(cancel_rx.recv_timeout(OWNER_EVENT_DEADLINE).unwrap(), 1);
    assert!(owner.wait_for_unconfirmed_with_timeout(OWNER_EVENT_DEADLINE));
    settle_tx.send(true).unwrap();
    assert_eq!(dropped_rx.recv_timeout(OWNER_EVENT_DEADLINE).unwrap(), 1);
    assert!(owner
        .wait_for_free_slots_with_timeout(super::owner::MAX_RESOURCE_SLOTS, OWNER_EVENT_DEADLINE));
}

#[test]
fn sentinel_probe_owner_drop_closes_without_retained_arc_cycle() {
    let owner = Arc::new(ProbeResourceOwner::new());
    let mut reservation = owner.reserve().unwrap();
    reservation.register_resource().unwrap();
    let (cancel_tx, cancel_rx) = mpsc::channel();
    let (settle_tx, settle_rx) = mpsc::channel();
    let (dropped_tx, dropped_rx) = mpsc::channel();
    drop(owner);
    reservation.retain(Box::new(event_resource(
        6, cancel_tx, settle_rx, dropped_tx, None,
    )));

    assert_eq!(cancel_rx.recv_timeout(OWNER_EVENT_DEADLINE).unwrap(), 6);
    settle_tx.send(true).unwrap();
    assert_eq!(dropped_rx.recv_timeout(OWNER_EVENT_DEADLINE).unwrap(), 6);
}

#[test]
fn sentinel_probe_owner_signalled_event_releases_exact_slot() {
    let owner = Arc::new(ProbeResourceOwner::new());
    let (cancel_tx, cancel_rx) = mpsc::channel();
    let (settle_tx, settle_rx) = mpsc::channel();
    let (dropped_tx, dropped_rx) = mpsc::channel();
    retain_fake(&owner, 2, cancel_tx, settle_rx, dropped_tx, None);
    assert_eq!(cancel_rx.recv_timeout(OWNER_EVENT_DEADLINE).unwrap(), 2);
    settle_tx.send(true).unwrap();
    assert_eq!(dropped_rx.recv_timeout(OWNER_EVENT_DEADLINE).unwrap(), 2);
    assert!(owner
        .wait_for_free_slots_with_timeout(super::owner::MAX_RESOURCE_SLOTS, OWNER_EVENT_DEADLINE));
    let snapshot = owner.snapshot();
    assert_eq!(snapshot.settled_total, 1);
    assert_eq!(snapshot.free, super::owner::MAX_RESOURCE_SLOTS);
}

#[test]
fn sentinel_probe_owner_unresolved_slot_does_not_starve_settled_slot() {
    let owner = Arc::new(ProbeResourceOwner::new());
    let (settle_a_tx, settle_a_rx) = mpsc::channel();
    let (settle_b_tx, settle_b_rx) = mpsc::channel();
    let (dropped_tx, dropped_rx) = mpsc::channel();
    let (cancel_a_tx, cancel_a_rx) = mpsc::channel();
    let (cancel_b_tx, cancel_b_rx) = mpsc::channel();
    retain_fake(
        &owner,
        3,
        cancel_a_tx,
        settle_a_rx,
        dropped_tx.clone(),
        None,
    );
    retain_fake(&owner, 4, cancel_b_tx, settle_b_rx, dropped_tx, None);
    assert_eq!(cancel_a_rx.recv_timeout(OWNER_EVENT_DEADLINE).unwrap(), 3);
    assert_eq!(cancel_b_rx.recv_timeout(OWNER_EVENT_DEADLINE).unwrap(), 4);

    settle_b_tx.send(true).unwrap();
    assert_eq!(dropped_rx.recv_timeout(OWNER_EVENT_DEADLINE).unwrap(), 4);
    assert!(owner.wait_for_free_slots_with_timeout(
        super::owner::MAX_RESOURCE_SLOTS - 1,
        OWNER_EVENT_DEADLINE
    ));
    let snapshot = owner.snapshot();
    assert_eq!(snapshot.settled_total, 1);
    assert_eq!(snapshot.unconfirmed, 1);
    assert_eq!(snapshot.free, super::owner::MAX_RESOURCE_SLOTS - 1);

    settle_a_tx.send(true).unwrap();
    assert_eq!(dropped_rx.recv_timeout(OWNER_EVENT_DEADLINE).unwrap(), 3);
    assert!(owner
        .wait_for_free_slots_with_timeout(super::owner::MAX_RESOURCE_SLOTS, OWNER_EVENT_DEADLINE));
}

#[test]
fn sentinel_probe_owner_callback_observes_state_without_owner_lock() {
    let owner = Arc::new(ProbeResourceOwner::new());
    let (cancel_tx, cancel_rx) = mpsc::channel();
    let (settle_tx, settle_rx) = mpsc::channel();
    let (dropped_tx, dropped_rx) = mpsc::channel();
    let (observed_tx, observed_rx) = mpsc::channel();
    retain_fake(
        &owner,
        5,
        cancel_tx,
        settle_rx,
        dropped_tx,
        Some((Arc::downgrade(&owner), observed_tx)),
    );
    assert_eq!(cancel_rx.recv_timeout(OWNER_EVENT_DEADLINE).unwrap(), 5);
    let snapshot = observed_rx.recv_timeout(OWNER_EVENT_DEADLINE).unwrap();
    assert_eq!(snapshot.unconfirmed, 1);
    settle_tx.send(true).unwrap();
    assert_eq!(dropped_rx.recv_timeout(OWNER_EVENT_DEADLINE).unwrap(), 5);
    assert!(owner
        .wait_for_free_slots_with_timeout(super::owner::MAX_RESOURCE_SLOTS, OWNER_EVENT_DEADLINE));
}

#[test]
fn sentinel_probe_windows_permanent_close_uncertainty_is_retained() {
    let owner = Arc::new(ProbeResourceOwner::new());
    let mut reservation = owner.reserve().unwrap();
    reservation.register_resource().unwrap();
    let event_waits = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicUsize::new(0));
    let primary = "probe_close_failed";
    let opaque_error = "opaque-close::0xDEADBEEF";
    reservation.retain(Box::new(PermanentCloseResource {
        event_waits: event_waits.clone(),
        dropped: dropped.clone(),
        primary,
        opaque_error,
    }));
    assert!(owner.wait_for_terminal_unconfirmed_with_timeout(OWNER_EVENT_DEADLINE));
    let snapshot = owner.snapshot();
    assert_eq!(event_waits.load(Ordering::SeqCst), 0);
    assert_eq!(snapshot.free, super::owner::MAX_RESOURCE_SLOTS - 1);
    assert_eq!(snapshot.unconfirmed, 1);
    assert_eq!(snapshot.settled_total, 0);
    assert!(snapshot
        .diagnostics
        .iter()
        .any(|item| item.as_str() == primary));
    assert!(snapshot
        .diagnostics
        .iter()
        .any(|item| item.as_str() == opaque_error));
    drop(owner);
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
}

struct FakeEventResource {
    id: usize,
    cancel: Sender<usize>,
    settlement: Receiver<bool>,
    dropped: Sender<usize>,
    observer: Option<(Weak<ProbeResourceOwner>, Sender<super::owner::Snapshot>)>,
}

fn event_resource(
    id: usize,
    cancel: Sender<usize>,
    settlement: Receiver<bool>,
    dropped: Sender<usize>,
    observer: Option<(Weak<ProbeResourceOwner>, Sender<super::owner::Snapshot>)>,
) -> FakeEventResource {
    FakeEventResource {
        id,
        cancel,
        settlement,
        dropped,
        observer,
    }
}

fn retain_fake(
    owner: &Arc<ProbeResourceOwner>,
    id: usize,
    cancel: Sender<usize>,
    settlement: Receiver<bool>,
    dropped: Sender<usize>,
    observer: Option<(Weak<ProbeResourceOwner>, Sender<super::owner::Snapshot>)>,
) {
    let mut reservation = owner.reserve().unwrap();
    reservation.register_resource().unwrap();
    reservation.retain(Box::new(event_resource(
        id, cancel, settlement, dropped, observer,
    )));
}

impl RetainedResources for FakeEventResource {
    fn request_cancel(&mut self) -> Vec<String> {
        if let Some((owner, observed)) = self.observer.take() {
            if let Some(owner) = owner.upgrade() {
                let _ = observed.send(owner.snapshot());
            }
        }
        let _ = self.cancel.send(self.id);
        Vec::new()
    }

    fn wait_for_settlement(&mut self) -> Settlement {
        match self.settlement.recv_timeout(OWNER_EVENT_DEADLINE) {
            Ok(true) => Settlement {
                primary: None,
                errors: Vec::new(),
                remains_unconfirmed: false,
            },
            _ => Settlement {
                primary: Some("fake_settlement_unconfirmed".into()),
                errors: Vec::new(),
                remains_unconfirmed: true,
            },
        }
    }
}

impl Drop for FakeEventResource {
    fn drop(&mut self) {
        let _ = self.dropped.send(self.id);
    }
}

struct PermanentCloseResource {
    event_waits: Arc<AtomicUsize>,
    dropped: Arc<AtomicUsize>,
    primary: &'static str,
    opaque_error: &'static str,
}

impl RetainedResources for PermanentCloseResource {
    fn request_cancel(&mut self) -> Vec<String> {
        Vec::new()
    }

    fn wait_for_settlement(&mut self) -> Settlement {
        let mut permanent = false;
        let error = super::owner::mark_close_uncertain(&mut permanent, self.opaque_error.into());
        if super::owner::settlement_event_required(false, permanent, false) {
            self.event_waits.fetch_add(1, Ordering::SeqCst);
        }
        Settlement {
            primary: Some(self.primary.into()),
            errors: vec![error],
            remains_unconfirmed: true,
        }
    }
}

impl Drop for PermanentCloseResource {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

fn completed_data(bytes: &[u8]) -> CaptureData {
    let mut data = CaptureData::default();
    data.append(bytes);
    data.mark_eof();
    data
}

fn test_status() -> std::process::ExitStatus {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(0)
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(0)
    }
}

#[test]
fn sentinel_probe_capture_typed_status_comes_from_exit_status_preserving_hash_input() {
    for code in [0, 7] {
        let output = super::fixture_output(code, b"exit=0\nopaque", b"err");
        assert_eq!(output.exit_code, Some(code));
        assert_eq!(output.success, code == 0);
        let mut bytes = code.to_le_bytes().to_vec();
        bytes.extend_from_slice(&(13u64).to_le_bytes());
        bytes.extend_from_slice(b"exit=0\nopaque");
        bytes.extend_from_slice(b"err");
        assert_eq!(output.bytes, bytes);
        assert_eq!(output.stdout.total_bytes, 13);
        assert_eq!(output.stdout.retained_bytes, 13);
        assert!(!output.stdout.truncated);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        let output = super::capture::assemble(
            std::process::ExitStatus::from_raw(libc::SIGTERM),
            completed_data(b"out"),
            completed_data(b"err"),
        );
        assert_eq!(output.exit_code, None);
        assert!(!output.success);
        assert_eq!(output.text, "exit=signal\nstdout:\nout\nstderr:\nerr");
        assert_eq!(&output.bytes[..4], &(-1i32).to_le_bytes());
    }
}

#[test]
fn sentinel_probe_owner_shutdown_delivers_only_registered_slot_cancellation() {
    let owner = Arc::new(ProbeResourceOwner::new());
    let signal = super::cancellation::Cancellation::default();
    let unrelated = super::cancellation::Cancellation::default();
    let mut reservation = owner.reserve().unwrap();
    reservation.register_cancellation(signal.clone()).unwrap();
    owner.close_admission();
    assert!(signal.load(Ordering::SeqCst));
    assert!(!unrelated.load(Ordering::SeqCst));
    reservation.release().unwrap();
    assert_eq!(owner.snapshot().free, super::owner::MAX_RESOURCE_SLOTS);
}

#[test]
fn sentinel_probe_readiness_handoff_preserves_default_deadlines() {
    let deadlines = super::process::Deadlines::start(PROBE_RUNTIME, PROBE_RUNTIME).unwrap();
    let signal = super::cancellation::Cancellation::default();
    let setup = signal.setup_deadlines(deadlines);
    let ready = signal.child_ready(setup).unwrap();
    assert_eq!(ready.receipt().admitted, deadlines.receipt().admitted);
    assert_eq!(ready.receipt().hard, deadlines.receipt().hard);
}

#[test]
fn sentinel_probe_readiness_handoff_uses_observed_operation_deadline() {
    let operation =
        super::process::Deadlines::start(Duration::from_secs(1), PROBE_RUNTIME).unwrap();
    let signal = super::cancellation::Cancellation::with_readiness(move || Ok(operation));
    let setup = signal.setup_deadlines(operation);
    assert_eq!(setup.duration, super::test_readiness::SETUP_DEADLINE);
    let ready = signal.child_ready(setup).unwrap();
    assert_eq!(ready.receipt().admitted, operation.receipt().admitted);
    assert_eq!(ready.receipt().hard, operation.receipt().hard);
    assert_eq!(ready.duration, Duration::from_secs(1));
}

#[test]
fn sentinel_probe_readiness_handoff_fails_on_missing_child_signal() {
    let signal = super::cancellation::Cancellation::with_readiness(|| Err("missing signal".into()));
    let deadlines = super::process::Deadlines::start(PROBE_RUNTIME, PROBE_RUNTIME).unwrap();
    assert_eq!(
        signal.child_ready(deadlines).err().as_deref(),
        Some("missing signal")
    );
}
