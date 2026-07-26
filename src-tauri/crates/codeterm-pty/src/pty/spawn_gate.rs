use std::sync::Arc;
use std::time::Instant;

use parking_lot::{Condvar, Mutex};

#[derive(Default)]
struct SpawnGateState {
    quiesced: bool,
    active: usize,
}

#[derive(Clone, Default)]
pub struct PtySpawnGate {
    inner: Arc<(Mutex<SpawnGateState>, Condvar)>,
}

pub struct PtySpawnPermit {
    gate: PtySpawnGate,
}

impl PtySpawnGate {
    pub fn acquire(&self) -> Result<PtySpawnPermit, String> {
        let mut state = self.inner.0.lock();
        if state.quiesced {
            return Err("PTY spawning is quiesced for shutdown".to_string());
        }
        state.active += 1;
        Ok(PtySpawnPermit { gate: self.clone() })
    }

    pub fn quiesce(&self) {
        self.inner.0.lock().quiesced = true;
    }

    pub fn wait_for_idle(&self, deadline: Instant) -> Result<(), String> {
        let mut state = self.inner.0.lock();
        while state.active != 0 {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return Err(format!(
                    "{} PTY spawn(s) did not finish before shutdown deadline",
                    state.active
                ));
            };
            if self.inner.1.wait_for(&mut state, remaining).timed_out() {
                return Err(format!(
                    "{} PTY spawn(s) did not finish before shutdown deadline",
                    state.active
                ));
            }
        }
        Ok(())
    }
}

impl Drop for PtySpawnPermit {
    fn drop(&mut self) {
        let mut state = self.gate.inner.0.lock();
        state.active = state.active.saturating_sub(1);
        self.gate.inner.1.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::PtySpawnGate;

    #[test]
    fn quiesce_rejects_new_spawns_and_waits_for_active_permits() {
        let gate = PtySpawnGate::default();
        let permit = gate.acquire().expect("first spawn permit");
        gate.quiesce();
        assert!(gate.acquire().is_err());

        let waiter = gate.clone();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            done_tx
                .send(
                    waiter.wait_for_idle(
                        std::time::Instant::now() + std::time::Duration::from_secs(1),
                    ),
                )
                .unwrap();
        });

        started_rx.recv().unwrap();
        assert!(matches!(
            done_rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
        drop(permit);
        assert!(done_rx.recv().unwrap().is_ok());
    }
}
