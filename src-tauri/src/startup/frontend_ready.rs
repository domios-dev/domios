use std::sync::OnceLock;
use std::time::Instant;

use parking_lot::{Condvar, Mutex};

#[derive(Default)]
struct FrontendReadySignal {
    state: Mutex<FrontendReadyState>,
    changed: Condvar,
}

#[derive(Default)]
struct FrontendReadyState {
    generation: u64,
    ready_generation: Option<u64>,
}

impl FrontendReadySignal {
    fn arm(&self) -> u64 {
        let mut state = self.state.lock();
        state.generation = state.generation.wrapping_add(1);
        state.ready_generation = None;
        self.changed.notify_all();
        state.generation
    }

    fn generation(&self) -> u64 {
        self.state.lock().generation
    }

    fn mark_ready(&self, generation: u64) -> bool {
        let mut state = self.state.lock();
        if state.generation == generation && state.ready_generation != Some(generation) {
            state.ready_generation = Some(generation);
            self.changed.notify_all();
            return true;
        }
        false
    }

    fn wait_until(&self, generation: u64, deadline: Instant) -> bool {
        let mut state = self.state.lock();
        loop {
            if state.generation == generation && state.ready_generation == Some(generation) {
                return true;
            }
            if state.generation != generation {
                return false;
            }
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };
            let timeout = self.changed.wait_for(&mut state, remaining);
            if timeout.timed_out()
                && (state.generation != generation || state.ready_generation != Some(generation))
            {
                return false;
            }
        }
    }
}

fn frontend_ready_signal() -> &'static FrontendReadySignal {
    static SIGNAL: OnceLock<FrontendReadySignal> = OnceLock::new();
    SIGNAL.get_or_init(FrontendReadySignal::default)
}

pub(crate) fn arm_frontend_readiness() -> u64 {
    frontend_ready_signal().arm()
}

pub(crate) fn frontend_readiness_generation() -> u64 {
    frontend_ready_signal().generation()
}

pub(crate) fn mark_frontend_ready(generation: u64) -> bool {
    frontend_ready_signal().mark_ready(generation)
}

pub(crate) fn wait_for_frontend_ready_until(generation: u64, deadline: Instant) -> bool {
    frontend_ready_signal().wait_until(generation, deadline)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn frontend_ready_event_wakes_armed_waiter() {
        let signal = std::sync::Arc::new(FrontendReadySignal::default());
        let generation = signal.arm();
        let waiter_signal = signal.clone();
        let (armed_tx, armed_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let waiter = std::thread::spawn(move || {
            armed_tx.send(()).unwrap();
            done_tx
                .send(waiter_signal.wait_until(generation, Instant::now() + Duration::from_secs(1)))
                .unwrap();
        });

        armed_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(signal.mark_ready(generation));

        assert!(done_rx.recv_timeout(Duration::from_secs(1)).unwrap());
        waiter.join().unwrap();
    }

    #[test]
    fn frontend_ready_deadline_is_not_success() {
        let signal = FrontendReadySignal::default();
        let generation = signal.arm();

        assert!(!signal.wait_until(generation, Instant::now()));
    }

    #[test]
    fn prior_frontend_generation_cannot_satisfy_rearmed_waiter() {
        let signal = FrontendReadySignal::default();
        let prior_generation = signal.arm();
        assert!(signal.mark_ready(prior_generation));
        let replacement_generation = signal.arm();

        assert!(!signal.mark_ready(prior_generation));
        assert!(!signal.wait_until(replacement_generation, Instant::now()));
    }
}
