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
        let (snapshot, error) = self
            .steps
            .lock()
            .unwrap()
            .pop_front()
            .expect("bounded injected listen port");
        if snapshot {
            self.progress.fetch_add(1, Ordering::AcqRel);
            self.state(ListenerState::Receiving);
        }
        Err(error)
    }
}

#[tokio::test]
async fn document_internal_reset_reconnects_without_external_wakeup_and_snapshot_resets_budget() {
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
    let watch = Watch::new(vec![(false, transient); 4]);
    assert_eq!(supervise_documents(&watch).await, Err(transient));
    assert_eq!(*watch.retries.lock().unwrap(), vec![1, 2, 3]);
    assert_eq!(
        watch.failures.lock().unwrap().last(),
        Some(&Error::ListenProtocol {
            kind: "listen_retries_exhausted"
        })
    );
    assert_eq!(
        watch.states.lock().unwrap().last(),
        Some(&ListenerState::Stopped)
    );
    for error in [
        Error::Firestore {
            code: 7,
            operation: "listen",
        },
        Error::Firestore {
            code: 16,
            operation: "listen",
        },
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
    for attempt in 1..=3 {
        let base = 500u64 << (attempt - 1);
        for jitter in [0, 1, u64::MAX] {
            let delay = reconnect_delay(attempt, jitter).as_millis();
            assert!((u128::from(base)..=u128::from(base + base / 2)).contains(&delay));
        }
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
