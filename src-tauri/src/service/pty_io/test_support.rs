use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub(crate) const TEST_WAIT_DEADLINE: Duration = Duration::from_secs(10);

pub(crate) fn recv<T>(receiver: &Receiver<T>, context: &'static str) -> T {
    receiver
        .recv_timeout(TEST_WAIT_DEADLINE)
        .unwrap_or_else(|_| panic!("timed out waiting for {context}"))
}

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

pub(crate) fn watchdog<F>(context: &'static str, deadline: Duration, test: F)
where
    F: FnOnce() + Send + 'static,
{
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let result = catch_unwind(AssertUnwindSafe(test));
        let _ = result_tx.send(result);
    });

    match result_rx.recv_timeout(deadline) {
        Ok(Ok(())) => join(worker, context).expect("watchdog worker panicked after reporting"),
        Ok(Err(payload)) => {
            let _ = join(worker, context);
            resume_unwind(payload);
        }
        Err(RecvTimeoutError::Timeout) => {
            panic!("test {context} exceeded its {deadline:?} watchdog")
        }
        Err(RecvTimeoutError::Disconnected) => {
            panic!("test {context} exited without reporting its result")
        }
    }
}

pub(crate) fn watchdog_10s<F>(context: &'static str, test: F)
where
    F: FnOnce() + Send + 'static,
{
    watchdog(context, TEST_WAIT_DEADLINE, test);
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
