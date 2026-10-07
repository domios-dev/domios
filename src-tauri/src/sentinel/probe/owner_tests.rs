use super::owner::{ProbeResourceOwner, RetainedResources, Settlement, MAX_RESOURCE_SLOTS};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

struct ExitEvidence {
    exited: Arc<AtomicBool>,
    io_complete: Arc<AtomicBool>,
    dropped: Arc<AtomicUsize>,
    event: Option<CaptureEvent>,
}

struct CaptureEvent {
    complete: Receiver<()>,
    waiting: Sender<()>,
    retired: Sender<()>,
}

struct CaptureHarness {
    exited: Arc<AtomicBool>,
    dropped: Arc<AtomicUsize>,
    complete: Sender<()>,
    waiting: Receiver<()>,
    retired: Receiver<()>,
}

fn pending_capture() -> (ExitEvidence, CaptureHarness) {
    let exited = Arc::new(AtomicBool::new(false));
    let dropped = Arc::new(AtomicUsize::new(0));
    let (complete, complete_rx) = mpsc::channel();
    let (waiting_tx, waiting) = mpsc::channel();
    let (retired_tx, retired) = mpsc::channel();
    (
        ExitEvidence {
            exited: exited.clone(),
            io_complete: Arc::new(AtomicBool::new(false)),
            dropped: dropped.clone(),
            event: Some(CaptureEvent {
                complete: complete_rx,
                waiting: waiting_tx,
                retired: retired_tx,
            }),
        },
        CaptureHarness {
            exited,
            dropped,
            complete,
            waiting,
            retired,
        },
    )
}

impl RetainedResources for ExitEvidence {
    fn processes_settled(&mut self) -> bool {
        self.exited.load(Ordering::SeqCst)
    }

    fn wait_for_capture_settlement(&mut self) -> Settlement {
        if let Some(event) = &self.event {
            event.waiting.send(()).unwrap();
            if event.complete.recv_timeout(Duration::from_secs(2)).is_ok() {
                self.io_complete.store(true, Ordering::SeqCst);
            }
        }
        self.wait_for_settlement()
    }

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
        if let Some(event) = &self.event {
            let _ = event.retired.send(());
        }
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
        event: None,
    }));
    assert_eq!(owner.snapshot().free, MAX_RESOURCE_SLOTS);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}

#[test]
fn stale_process_releases_capacity_while_capture_waits_for_its_event() {
    let owner = ProbeResourceOwner::new();
    let (resources, capture) = pending_capture();
    let mut guard = owner.reserve().unwrap();
    guard.register_resource().unwrap();
    guard.retain(Box::new(resources));
    assert!(owner.wait_for_terminal_unconfirmed_with_timeout(Duration::from_secs(2)));
    let guards = (0..MAX_RESOURCE_SLOTS - 1)
        .map(|_| owner.reserve().unwrap())
        .collect::<Vec<_>>();
    assert!(matches!(
        owner.reserve(),
        Err(super::owner::AdmissionError::Full)
    ));
    capture.exited.store(true, Ordering::SeqCst);
    let mut reclaimed = owner.reserve().unwrap();
    reclaimed.register_resource().unwrap();
    capture
        .waiting
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    assert_eq!(owner.snapshot().reserved, MAX_RESOURCE_SLOTS - 1);
    assert_eq!(owner.snapshot().active, 1);
    assert_eq!(owner.snapshot().unconfirmed, 0);
    assert_eq!(owner.snapshot().captures, 1);
    assert_eq!(capture.dropped.load(Ordering::SeqCst), 0);
    capture.complete.send(()).unwrap();
    capture
        .retired
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    assert!(owner.wait_for_capture_count_with_timeout(0, Duration::from_secs(2)));
    assert_eq!(capture.dropped.load(Ordering::SeqCst), 1);
    assert_eq!(owner.snapshot().active, 1);
    assert_eq!(owner.snapshot().reserved, MAX_RESOURCE_SLOTS - 1);
    reclaimed.resource_settled().unwrap();
    drop(reclaimed);
    drop(guards);
    assert_eq!(owner.snapshot().free, MAX_RESOURCE_SLOTS);
}

#[test]
fn pending_capture_count_does_not_exhaust_process_admission() {
    let owner = ProbeResourceOwner::new();
    let mut captures = Vec::new();
    for _ in 0..MAX_RESOURCE_SLOTS + 1 {
        let (resources, capture) = pending_capture();
        capture.exited.store(true, Ordering::SeqCst);
        let mut guard = owner.reserve().unwrap();
        guard.register_resource().unwrap();
        guard.retain(Box::new(resources));
        captures.push(capture);
    }
    assert_eq!(owner.snapshot().free, MAX_RESOURCE_SLOTS);
    assert_eq!(owner.snapshot().captures, MAX_RESOURCE_SLOTS + 1);
    let mut live = owner.reserve().unwrap();
    live.register_resource().unwrap();
    for capture in &captures {
        assert_eq!(capture.dropped.load(Ordering::SeqCst), 0);
        capture.complete.send(()).unwrap();
    }
    for capture in captures {
        capture
            .waiting
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        capture
            .retired
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
    }
    assert!(owner.wait_for_capture_count_with_timeout(0, Duration::from_secs(2)));
    assert_eq!(owner.snapshot().active, 1);
    live.resource_settled().unwrap();
    live.release().unwrap();
    assert_eq!(owner.snapshot().free, MAX_RESOURCE_SLOTS);
}

#[test]
fn owner_drop_keeps_pending_capture_until_exact_completion() {
    let owner = Arc::new(ProbeResourceOwner::new());
    let weak = Arc::downgrade(&owner);
    let (resources, capture) = pending_capture();
    capture.exited.store(true, Ordering::SeqCst);
    let mut guard = owner.reserve().unwrap();
    guard.register_resource().unwrap();
    guard.retain(Box::new(resources));
    capture
        .waiting
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    assert_eq!(owner.snapshot().free, MAX_RESOURCE_SLOTS);
    drop(owner);
    assert!(weak.upgrade().is_none());
    assert_eq!(capture.dropped.load(Ordering::SeqCst), 0);
    capture.complete.send(()).unwrap();
    capture
        .retired
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    assert_eq!(capture.dropped.load(Ordering::SeqCst), 1);
}

#[test]
fn owner_drop_cancels_released_probe_and_accepts_its_late_capture_transfer() {
    let owner = Arc::new(ProbeResourceOwner::new());
    let weak = Arc::downgrade(&owner);
    let cancellation = super::cancellation::Cancellation::default();
    let mut guard = owner.reserve().unwrap();
    guard.register_cancellation(cancellation.clone()).unwrap();
    guard.register_resource().unwrap();
    guard.release_process_admission();
    drop(owner);
    assert!(weak.upgrade().is_none());
    assert!(cancellation.load(Ordering::SeqCst));
    let (resources, capture) = pending_capture();
    capture.exited.store(true, Ordering::SeqCst);
    guard.retain(Box::new(resources));
    capture
        .waiting
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    assert_eq!(capture.dropped.load(Ordering::SeqCst), 0);
    capture.complete.send(()).unwrap();
    capture
        .retired
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    assert_eq!(capture.dropped.load(Ordering::SeqCst), 1);
}

#[test]
fn released_admission_can_transfer_capture_without_touching_its_replacement() {
    let owner = ProbeResourceOwner::new();
    let mut old = owner.reserve().unwrap();
    old.register_resource().unwrap();
    old.release_process_admission();
    let mut current = owner.reserve().unwrap();
    current.register_resource().unwrap();
    let (resources, capture) = pending_capture();
    capture.exited.store(true, Ordering::SeqCst);
    old.retain(Box::new(resources));
    capture
        .waiting
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    assert_eq!(owner.snapshot().active, 1);
    assert_eq!(owner.snapshot().captures, 1);
    assert_eq!(capture.dropped.load(Ordering::SeqCst), 0);
    capture.complete.send(()).unwrap();
    capture
        .retired
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    assert!(owner.wait_for_capture_count_with_timeout(0, Duration::from_secs(2)));
    assert_eq!(owner.snapshot().active, 1);
    current.resource_settled().unwrap();
    current.release().unwrap();
    assert_eq!(owner.snapshot().free, MAX_RESOURCE_SLOTS);
}

#[test]
fn old_resource_settlement_cannot_modify_concurrently_reused_admission() {
    let owner = ProbeResourceOwner::new();
    let mut old = owner.reserve().unwrap();
    old.register_resource().unwrap();
    old.release_process_admission();
    let mut current = owner.reserve().unwrap();
    current.register_resource().unwrap();
    std::thread::scope(|scope| {
        let worker = scope.spawn(move || {
            old.register_resource().unwrap();
            old.resource_settled().unwrap();
            old.resource_settled().unwrap();
            old.release().unwrap();
        });
        worker.join().unwrap();
        assert_eq!(owner.snapshot().active, 1);
        assert_eq!(owner.snapshot().free, MAX_RESOURCE_SLOTS - 1);
        current.resource_settled().unwrap();
        current.release().unwrap();
    });
    assert_eq!(owner.snapshot().free, MAX_RESOURCE_SLOTS);
}
