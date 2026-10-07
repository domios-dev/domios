use super::owner::{ProbeResourceOwner, RetainedResources, Settlement, MAX_RESOURCE_SLOTS};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

struct ExitEvidence {
    exited: Arc<AtomicBool>,
    io_complete: Arc<AtomicBool>,
    dropped: Arc<AtomicUsize>,
}

impl RetainedResources for ExitEvidence {
    fn request_cancel(&mut self) -> Vec<String> {
        Vec::new()
    }

    fn wait_for_settlement(&mut self) -> Settlement {
        Settlement {
            primary: Some("probe_capture_failed".into()),
            errors: Vec::new(),
            remains_unconfirmed: !self.try_settle(),
        }
    }

    fn try_settle(&mut self) -> bool {
        self.exited.load(Ordering::SeqCst) && self.io_complete.load(Ordering::SeqCst)
    }
}

impl Drop for ExitEvidence {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn guard_releases_each_terminal_outcome() {
    let owner = Arc::new(ProbeResourceOwner::new());
    for (reason, resources) in [
        ("probe_spawn_failed", 0),
        ("probe_capture_failed", 2),
        ("probe_timeout", 2),
        ("probe_cancelled", 2),
    ] {
        let failed = super::admit_then::<()>(&owner, |mut guard| {
            for _ in 0..resources {
                guard.register_resource().unwrap();
            }
            for _ in 0..resources {
                guard.resource_settled().unwrap();
            }
            Err(super::process::ProbeFailure::new(reason))
        });
        assert_eq!(failed.unwrap_err().primary, reason);
        assert_eq!(owner.snapshot().free, MAX_RESOURCE_SLOTS);
        assert_eq!(owner.snapshot().unconfirmed, 0);
    }
    super::admit_then(&owner, |mut guard| {
        guard.register_resource().unwrap();
        guard.resource_settled().unwrap();
        Ok(())
    })
    .unwrap();
    assert_eq!(owner.snapshot().free, MAX_RESOURCE_SLOTS);
}

#[test]
fn terminal_evidence_releases_slot_before_returning_failure() {
    let owner = ProbeResourceOwner::new();
    let dropped = Arc::new(AtomicUsize::new(0));
    let mut guard = owner.reserve().unwrap();
    guard.register_resource().unwrap();
    guard.retain(Box::new(ExitEvidence {
        exited: Arc::new(AtomicBool::new(true)),
        io_complete: Arc::new(AtomicBool::new(true)),
        dropped: dropped.clone(),
    }));
    assert_eq!(owner.snapshot().free, MAX_RESOURCE_SLOTS);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}

#[test]
fn stale_reservation_reclaimed_only_after_exit_and_io_evidence() {
    let owner = ProbeResourceOwner::new();
    let exited = Arc::new(AtomicBool::new(false));
    let io_complete = Arc::new(AtomicBool::new(false));
    let dropped = Arc::new(AtomicUsize::new(0));
    let mut guard = owner.reserve().unwrap();
    guard.register_resource().unwrap();
    guard.retain(Box::new(ExitEvidence {
        exited: exited.clone(),
        io_complete: io_complete.clone(),
        dropped: dropped.clone(),
    }));
    assert!(owner.wait_for_terminal_unconfirmed_with_timeout(Duration::from_secs(2)));
    let guards = (0..MAX_RESOURCE_SLOTS - 1)
        .map(|_| owner.reserve().unwrap())
        .collect::<Vec<_>>();
    assert!(matches!(
        owner.reserve(),
        Err(super::owner::AdmissionError::Full)
    ));
    exited.store(true, Ordering::SeqCst);
    assert!(matches!(
        owner.reserve(),
        Err(super::owner::AdmissionError::Full)
    ));
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    io_complete.store(true, Ordering::SeqCst);
    let reclaimed = owner.reserve().unwrap();
    assert_eq!(owner.snapshot().reserved, MAX_RESOURCE_SLOTS);
    assert_eq!(owner.snapshot().unconfirmed, 0);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    drop(reclaimed);
    drop(guards);
    assert_eq!(owner.snapshot().free, MAX_RESOURCE_SLOTS);
}
