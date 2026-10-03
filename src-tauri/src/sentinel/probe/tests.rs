use super::capture::{self, CaptureData, CompletionFacts};
use super::owner::{ProbeResourceOwner, RetainedResources, Settlement};
use super::process::{cancellation_target, Deadlines, LeaderOwnership, WindowsLaunchState};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

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
fn sentinel_probe_original_deadline_is_not_renewed() {
    let admitted = Instant::now();
    let deadlines = Deadlines::at(
        Duration::from_secs(9),
        Duration::from_secs(3),
        admitted,
    )
    .unwrap();
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

    let short = Deadlines::at(Duration::from_millis(100), Duration::from_secs(3), admitted)
        .unwrap();
    assert_eq!(short.operation, admitted + Duration::from_millis(75));
    assert_eq!(short.hard, admitted + Duration::from_millis(100));
}

#[test]
fn sentinel_probe_resource_owner_bounds_and_retains_unconfirmed() {
    let owner = Arc::new(ProbeResourceOwner::new());
    let mut reservations = (0..super::owner::MAX_RESOURCE_SLOTS)
        .map(|_| owner.reserve().unwrap())
        .collect::<Vec<_>>();
    assert!(owner.reserve().is_err());
    for reservation in reservations.drain(..) {
        reservation.release().unwrap();
    }
    owner.wait_for_free_slots(super::owner::MAX_RESOURCE_SLOTS);

    let dropped = Arc::new(AtomicUsize::new(0));
    let mut reservation = owner.reserve().unwrap();
    reservation.register_resource().unwrap();
    reservation.retain(Box::new(UnconfirmedResource {
        dropped: dropped.clone(),
    }));
    owner.wait_for_unconfirmed();
    let snapshot = owner.snapshot();
    assert_eq!(snapshot.unconfirmed, 1);
    assert_eq!(snapshot.free, super::owner::MAX_RESOURCE_SLOTS - 1);
    assert_eq!(snapshot.settled_total, 0);
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
}

struct UnconfirmedResource {
    dropped: Arc<AtomicUsize>,
}

impl RetainedResources for UnconfirmedResource {
    fn request_cancel(&mut self) -> Vec<String> {
        Vec::new()
    }

    fn wait_for_settlement(&mut self) -> Settlement {
        Settlement {
            errors: Vec::new(),
            remains_unconfirmed: true,
        }
    }
}

impl Drop for UnconfirmedResource {
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
