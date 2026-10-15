use std::sync::mpsc;
use std::sync::{Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub(crate) use super::send::readiness::PublicInputReadyDeadline;

pub(crate) const TEST_WAIT_DEADLINE: Duration = Duration::from_secs(10);

pub(crate) fn join<T: Send + 'static>(
    handle: JoinHandle<T>,
    context: &'static str,
) -> thread::Result<T> {
    let (done_tx, done_rx) = mpsc::sync_channel(1);
    let _joiner = thread::spawn(move || {
        let _ = done_tx.send(handle.join());
    });
    done_rx
        .recv_timeout(TEST_WAIT_DEADLINE)
        .unwrap_or_else(|_| panic!("timed out joining {context}"))
}

#[derive(Default)]
struct BarrierState {
    generation: usize,
    waiting: usize,
    broken: bool,
}

pub(crate) struct DeadlineBarrier {
    parties: usize,
    state: Mutex<BarrierState>,
    changed: Condvar,
}

impl DeadlineBarrier {
    pub(crate) fn new(parties: usize) -> Self {
        assert!(parties > 0);
        Self {
            parties,
            state: Mutex::new(BarrierState::default()),
            changed: Condvar::new(),
        }
    }

    pub(crate) fn wait(&self, context: &'static str) {
        let deadline = Instant::now() + TEST_WAIT_DEADLINE;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(!state.broken, "barrier {context} already timed out");
        let generation = state.generation;
        state.waiting += 1;
        if state.waiting == self.parties {
            state.waiting = 0;
            state.generation += 1;
            self.changed.notify_all();
            return;
        }

        loop {
            if state.broken {
                panic!("barrier {context} timed out");
            }
            if state.generation != generation {
                return;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                state.broken = true;
                self.changed.notify_all();
                panic!("barrier {context} timed out");
            }
            let (next, result) = self
                .changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state = next;
            if result.timed_out() && state.generation == generation {
                state.broken = true;
                self.changed.notify_all();
                panic!("barrier {context} timed out");
            }
        }
    }
}
