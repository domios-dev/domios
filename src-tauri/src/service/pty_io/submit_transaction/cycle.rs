use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, OnceLock, Weak};
use std::time::Instant;

use parking_lot::Mutex;

pub(super) struct SubmitCycle {
    generation: AtomicU64,
    waiter: Mutex<Option<(u64, Sender<()>)>>,
}

impl SubmitCycle {
    fn new() -> Self {
        Self {
            generation: AtomicU64::new(0),
            waiter: Mutex::new(None),
        }
    }

    fn reset(&self) -> u64 {
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        if let Some((_, waiter)) = self.waiter.lock().take() {
            let _ = waiter.send(());
        }
        generation
    }

    pub(super) fn is_current(&self, generation: u64) -> bool {
        self.generation.load(Ordering::Acquire) == generation
    }

    pub(super) fn install_waiter(&self, generation: u64, sender: Sender<()>) {
        let mut waiter = self.waiter.lock();
        if !self.is_current(generation) {
            drop(waiter);
            let _ = sender.send(());
            return;
        }
        *waiter = Some((generation, sender));
    }

    pub(super) fn wait_for_wake(
        &self,
        generation: u64,
        deadline: Instant,
        receiver: &Receiver<()>,
    ) -> bool {
        if !self.is_current(generation) {
            return false;
        }
        if let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            let _ = receiver.recv_timeout(remaining);
        }
        self.is_current(generation)
    }

    pub(super) fn clear_waiter(&self, generation: u64) {
        let mut waiter = self.waiter.lock();
        if waiter
            .as_ref()
            .is_some_and(|(waiter_generation, _)| *waiter_generation == generation)
        {
            waiter.take();
        }
    }

    fn wake(&self) {
        if let Some((_, sender)) = self.waiter.lock().as_ref() {
            let _ = sender.send(());
        }
    }
}

fn submit_cycles() -> &'static Mutex<HashMap<String, Weak<SubmitCycle>>> {
    static CYCLES: OnceLock<Mutex<HashMap<String, Weak<SubmitCycle>>>> = OnceLock::new();
    CYCLES.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Clone)]
pub(crate) struct SubmitCycleClaim {
    pub(super) pty_id: String,
    pub(super) cycle: Arc<SubmitCycle>,
    pub(super) generation: u64,
}

impl SubmitCycleClaim {
    pub(crate) fn pty_id(&self) -> &str {
        &self.pty_id
    }

    #[cfg(test)]
    pub(super) fn is_current(&self) -> bool {
        self.cycle.is_current(self.generation)
    }
}

pub(crate) fn claim_submit_cycle(pty_id: &str) -> SubmitCycleClaim {
    let cycle = {
        let mut cycles = submit_cycles().lock();
        if let Some(cycle) = cycles.get(pty_id).and_then(Weak::upgrade) {
            cycle
        } else {
            let cycle = Arc::new(SubmitCycle::new());
            cycles.insert(pty_id.to_string(), Arc::downgrade(&cycle));
            cycle
        }
    };
    let generation = cycle.reset();
    log::info!(
        "[submit-cycle] claim pty={} generation={}",
        pty_id,
        generation
    );
    SubmitCycleClaim {
        pty_id: pty_id.to_string(),
        cycle,
        generation,
    }
}

pub(crate) fn wake_submit_cycle(pty_id: &str) {
    let cycle = submit_cycles().lock().get(pty_id).and_then(Weak::upgrade);
    if let Some(cycle) = cycle {
        cycle.wake();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_turn_event_wakes_the_bounded_submit_wait() {
        let pty_id = "structured-turn-wake";
        let claim = claim_submit_cycle(pty_id);
        let (sender, receiver) = std::sync::mpsc::channel();
        claim.cycle.install_waiter(claim.generation, sender);

        wake_submit_cycle(pty_id);

        assert!(receiver.try_recv().is_ok());
    }
}

pub(super) fn retire_submit_cycle(pty_id: &str, cycle: &Arc<SubmitCycle>) {
    let mut cycles = submit_cycles().lock();
    let current_is_cycle = cycles
        .get(pty_id)
        .and_then(Weak::upgrade)
        .is_some_and(|current| Arc::ptr_eq(&current, cycle));
    if current_is_cycle && Arc::strong_count(cycle) == 1 {
        cycles.remove(pty_id);
    }
}
