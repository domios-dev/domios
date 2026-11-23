//! Latched scheduler wake: a notification raised while the scheduler is busy
//! ticking is kept and ends its next wait immediately instead of being lost.

use std::time::Duration;

use parking_lot::{Condvar, Mutex};

#[derive(Default)]
pub struct CronWakeSignal {
    generation: Mutex<u64>,
    changed: Condvar,
}

impl CronWakeSignal {
    pub fn notify_all(&self) {
        *self.generation.lock() += 1;
        self.changed.notify_all();
    }

    /// The generation a later `wait_after` compares against; read it before ticking.
    pub fn observe(&self) -> u64 {
        *self.generation.lock()
    }

    /// Waits until a notification newer than `observed` or the timeout; true when notified.
    pub fn wait_after(&self, observed: u64, timeout: Duration) -> bool {
        let mut generation = self.generation.lock();
        if *generation != observed {
            return true;
        }
        let _ =
            self.changed
                .wait_while_for(&mut generation, |current| *current == observed, timeout);
        *generation != observed
    }
}

#[cfg(test)]
mod tests;
