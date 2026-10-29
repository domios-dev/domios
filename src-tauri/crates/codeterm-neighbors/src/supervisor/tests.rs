use super::*;
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};

struct Watch {
    steps: Mutex<VecDeque<(bool, Error)>>,
    retries: Mutex<Vec<usize>>,
    failures: Mutex<Vec<Error>>,
    states: Mutex<Vec<ListenerState>>,
    progress: AtomicU64,
}
impl Watch {
    fn new(steps: Vec<(bool, Error)>) -> Self {
        Self {
            steps: Mutex::new(steps.into()),
            retries: Mutex::new(vec![]),
            failures: Mutex::new(vec![]),
            states: Mutex::new(vec![]),
            progress: AtomicU64::new(0),
        }
    }
}
#[async_trait]
impl ReceptionEvents for Watch {
    fn failure(&self, error: Error) {
        self.failures.lock().unwrap().push(error);
    }
    fn state(&self, state: ListenerState) {
        self.states.lock().unwrap().push(state);
    }
    async fn retry_after_failure(&self, attempt: usize) -> Result<()> {
        self.retries.lock().unwrap().push(attempt);
        Ok(())
    }
}
#[async_trait]
impl DocumentSession for Watch {
    fn progress(&self) -> u64 {
        self.progress.load(Ordering::Acquire)
    }
    async fn stopped(&self) {
        std::future::pending().await
    }
    async fn listen(&self) -> Result<()> {
        let (stable_progress, error) = self
            .steps
            .lock()
            .unwrap()
            .pop_front()
            .expect("bounded injected listen port");
        self.state(ListenerState::Receiving);
        if stable_progress {
            self.progress.fetch_add(1, Ordering::AcqRel);
        }
        Err(error)
    }
}

#[tokio::test]
async fn document_internal_reset_reconnects_without_external_wakeup_and_stable_progress_resets_budget(
) {
    let transient = Error::Firestore {
        code: 13,
        operation: "control.listen.read",
    };
    let mut steps = vec![(false, transient); 3];
    steps.push((true, transient));
    steps.extend(vec![(false, transient); 2]);
    steps.push((false, Error::Revoked));
    let watch = Watch::new(steps);
    assert_eq!(supervise_documents(&watch).await, Err(Error::Revoked));
    assert_eq!(*watch.retries.lock().unwrap(), vec![1, 2, 3, 1, 2, 3]);
    assert!(watch
        .states
        .lock()
        .unwrap()
        .contains(&ListenerState::Reconnecting));
}

#[tokio::test]
async fn document_retries_exhaust_visibly_and_terminal_auth_never_reconnects() {
    let transient = Error::Firestore {
        code: 14,
        operation: "registry.listen.read",
    };
    let mut failures = vec![(false, transient); 12];
    failures.push((false, Error::Revoked));
    let watch = Watch::new(failures);
    assert_eq!(supervise_documents(&watch).await, Err(Error::Revoked));
    assert_eq!(*watch.retries.lock().unwrap(), (1..=12).collect::<Vec<_>>());
    assert_eq!(watch.failures.lock().unwrap().last(), Some(&Error::Revoked));
    assert_eq!(
        watch.states.lock().unwrap().last(),
        Some(&ListenerState::Stopped)
    );
    for error in [
        Error::Firestore {
            code: 7,
            operation: "listen",
        },
        Error::Locked,
        Error::PermissionDenied,
        Error::SessionRevoked,
        Error::Revoked,
        Error::Disabled,
        Error::NotReceiver,
    ] {
        let watch = Watch::new(vec![(false, error)]);
        assert_eq!(supervise_documents(&watch).await, Err(error));
        assert!(watch.retries.lock().unwrap().is_empty());
    }
}

#[test]
fn backoff_has_bounded_exponential_jitter_and_only_transport_errors_retry() {
    for (attempt, ceiling) in [
        (1, 500),
        (2, 1000),
        (3, 2000),
        (10, 256000),
        (11, 300000),
        (usize::MAX, 300000),
    ] {
        assert_eq!(reconnect_delay(attempt, 0).as_millis(), u128::from(ceiling));
        assert_eq!(
            reconnect_delay(attempt, ceiling).as_millis(),
            u128::from(ceiling)
        );
        assert!(
            reconnect_delay(attempt, u64::MAX).as_millis()
                <= u128::from((ceiling * 2).min(300_000))
        );
    }
    assert!(retryable(Error::Transport));
    for kind in [
        "stream_closed",
        "request_stream_closed",
        "existence_filter_mismatch",
        "target_removed",
    ] {
        assert!(retryable(Error::ListenProtocol { kind }));
    }
    for code in [4, 10, 13, 14] {
        assert!(retryable(Error::Firestore {
            code,
            operation: "listen"
        }));
    }
    assert!(!retryable(Error::ListenProtocol {
        kind: "response_missing"
    }));
}

struct Clock(Mutex<Vec<std::time::Duration>>);
#[async_trait]
impl RetryClock for Clock {
    fn jitter(&self) -> u64 {
        300_000
    }
    async fn wait(&self, delay: std::time::Duration) {
        self.0.lock().unwrap().push(delay);
    }
}

#[tokio::test]
async fn retry_clock_receives_one_bounded_deadline_without_sleeping() {
    let clock = Clock(Mutex::new(vec![]));
    reconnect_backoff_with(&clock, usize::MAX).await;
    assert_eq!(
        *clock.0.lock().unwrap(),
        vec![std::time::Duration::from_secs(300)]
    );
}

#[test]
fn stream_health_requires_incremental_progress_after_a_stable_snapshot() {
    use std::time::Duration;
    let mut health = StreamHealth::default();
    assert!(!health.observe(Duration::ZERO, false));
    assert!(!health.observe(Duration::from_secs(29), true));
    assert!(!health.observe(Duration::from_secs(300), false));
    assert!(health.observe(Duration::from_secs(300), true));
    let mut delayed_snapshot = StreamHealth::default();
    assert!(!delayed_snapshot.observe(Duration::from_secs(1000), true));
    assert!(!delayed_snapshot.observe(Duration::from_secs(1029), true));
    assert!(delayed_snapshot.observe(Duration::from_secs(1030), true));
}

#[test]
fn hundreds_of_failures_have_a_positive_nondecreasing_floor_and_bounded_admissions() {
    let mut elapsed = std::time::Duration::ZERO;
    let mut previous = std::time::Duration::ZERO;
    let mut first_hour = 1;
    for attempt in 1..=500 {
        let delay = reconnect_delay(attempt, (attempt as u64).wrapping_mul(7919));
        assert!(delay >= previous);
        assert!(delay >= std::time::Duration::from_millis(500));
        assert!(delay <= std::time::Duration::from_secs(300));
        previous = delay;
        elapsed += delay;
        if elapsed < std::time::Duration::from_secs(3600) {
            first_hour += 1;
        }
    }
    assert!(first_hour <= 24, "{first_hour}");
}

#[tokio::test]
async fn repeated_initial_document_snapshots_never_reset_the_failure_budget() {
    let mut snapshots = vec![(false, Error::Transport); 500];
    snapshots.push((false, Error::Revoked));
    let watch = Watch::new(snapshots);
    assert_eq!(supervise_documents(&watch).await, Err(Error::Revoked));
    assert_eq!(
        *watch.retries.lock().unwrap(),
        (1..=500).collect::<Vec<_>>()
    );
    assert_eq!(
        watch
            .states
            .lock()
            .unwrap()
            .iter()
            .filter(|state| **state == ListenerState::Receiving)
            .count(),
        501
    );
}
