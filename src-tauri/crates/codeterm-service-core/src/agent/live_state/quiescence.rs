use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};

use crate::agent::offline_state_classifier::{STUCK_WORKING_IDLE_MS, WAIT_MS, WORKING_GRACE_MS};
use crate::service::{ServiceContext, ServiceState};

static WAKE: OnceLock<Arc<Wake>> = OnceLock::new();

#[derive(Default)]
struct Deadlines(HashMap<String, Instant>);

impl Deadlines {
    fn set(&mut self, pty_id: &str, deadline: Option<Instant>) {
        if let Some(deadline) = deadline {
            self.0.insert(pty_id.to_string(), deadline);
        } else {
            self.0.remove(pty_id);
        }
    }

    fn next(&self) -> Option<Instant> {
        self.0.values().copied().min()
    }

    fn take_due(&mut self, now: Instant) -> Vec<String> {
        let mut due = Vec::new();
        self.0.retain(|pty_id, deadline| {
            if *deadline <= now {
                due.push(pty_id.clone());
                false
            } else {
                true
            }
        });
        due
    }
}

#[derive(Default)]
struct Wake {
    deadlines: Mutex<Deadlines>,
    changed: Condvar,
}

fn next_delay(byte_idle_ms: u64, semantic_idle_ms: u64) -> Option<Duration> {
    [byte_idle_ms, semantic_idle_ms]
        .into_iter()
        .flat_map(|age| {
            [WAIT_MS, WORKING_GRACE_MS, STUCK_WORKING_IDLE_MS]
                .into_iter()
                .filter_map(move |boundary| boundary.checked_sub(age).filter(|delay| *delay > 0))
        })
        .min()
        .map(Duration::from_millis)
}

fn next_deadline(
    now: Instant,
    quiescence: Option<(u64, u64)>,
    suppression: Option<Instant>,
) -> Option<Instant> {
    quiescence
        .and_then(|(byte_idle, semantic_idle)| next_delay(byte_idle, semantic_idle))
        .map(|delay| now + delay)
        .into_iter()
        .chain(suppression)
        .min()
}

pub(crate) fn schedule(svc: &ServiceContext, pty_id: &str) {
    let Some(wake) = WAKE.get() else { return };
    let now = Instant::now();
    let quiescence = svc.agent_tracker.lock().quiescence(pty_id, now);
    let deadline = next_deadline(
        now,
        quiescence,
        ServiceContext::agent_stop_suppression_deadline(pty_id),
    );
    let mut deadlines = wake.deadlines.lock();
    let previous = deadlines.next();
    deadlines.set(pty_id, deadline);
    if deadlines.next() != previous {
        wake.changed.notify_one();
    }
}

pub(super) fn start(svc: ServiceState) {
    let wake = Arc::new(Wake::default());
    if WAKE.set(wake.clone()).is_err() {
        return;
    }
    codeterm_util::named_thread::spawn("agent-live-state", move || {
        let pty_ids = svc.agent_tracker.lock().all_ids();
        for pty_id in pty_ids {
            svc.recompute_agent_state(&pty_id);
            svc.run_watcher.complete_via_semantic_idle(&svc, &pty_id);
        }
        loop {
            let due = {
                let mut deadlines = wake.deadlines.lock();
                loop {
                    let now = Instant::now();
                    let due = deadlines.take_due(now);
                    if !due.is_empty() {
                        break due;
                    }
                    if let Some(deadline) = deadlines.next() {
                        wake.changed
                            .wait_for(&mut deadlines, deadline.saturating_duration_since(now));
                    } else {
                        wake.changed.wait(&mut deadlines);
                    }
                }
            };
            for pty_id in due {
                svc.recompute_agent_state(&pty_id);
                svc.run_watcher.complete_via_semantic_idle(&svc, &pty_id);
            }
        }
    });
}

#[cfg(test)]
mod tests;
