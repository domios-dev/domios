use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use super::pane_status::PaneStatus;

pub struct PaneStatusTracker {
    statuses: HashMap<String, PaneStatusEntry>,
    signals: HashMap<String, Arc<PaneStatusSignal>>,
}

#[derive(Default)]
struct PaneStatusSignalState {
    generation: u64,
    closed: bool,
}

#[derive(Default)]
pub struct PaneStatusSignal {
    state: Mutex<PaneStatusSignalState>,
    changed: Condvar,
}

impl PaneStatusSignal {
    pub fn generation(&self) -> u64 {
        self.state.lock().unwrap().generation
    }

    fn mark_changed(&self) {
        let mut state = self.state.lock().unwrap();
        state.generation = state.generation.wrapping_add(1);
        self.changed.notify_all();
    }

    fn close(&self) {
        let mut state = self.state.lock().unwrap();
        state.closed = true;
        self.changed.notify_all();
    }

    pub fn wait_after(&self, generation: u64, timeout: Duration) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.generation != generation {
            return true;
        }
        if state.closed {
            return false;
        }
        state = self.changed.wait_timeout(state, timeout).unwrap().0;
        !state.closed && state.generation != generation
    }
}

struct PaneStatusEntry {
    status: PaneStatus,
    last_output_hash: [u8; 32],
    /// Semantic clock origin: reset only when the (status-bar-excluded) screen
    /// content changes. Feeds `semantic_idle_ms`.
    last_output_time: Instant,
    /// Byte clock origin: reset on every output chunk. Feeds `byte_idle_ms`.
    last_byte_time: Instant,
    #[allow(dead_code)]
    last_check_time: Instant,
}

impl PaneStatusTracker {
    pub fn new() -> Self {
        Self {
            statuses: HashMap::new(),
            signals: HashMap::new(),
        }
    }

    pub fn arm_status_change_signal(&mut self, pty_id: &str) -> Arc<PaneStatusSignal> {
        self.signals
            .entry(pty_id.to_string())
            .or_insert_with(|| Arc::new(PaneStatusSignal::default()))
            .clone()
    }

    fn notify_if_changed(&self, pty_id: &str, previous: PaneStatus, current: PaneStatus) {
        if previous != current {
            if let Some(signal) = self.signals.get(pty_id) {
                signal.mark_changed();
            }
        }
    }

    /// Update status based on new PTY output. Returns `true` when the screen
    /// content actually changed (i.e. real activity, not a cursor blink or
    /// TUI repaint with identical content). Callers can use the return value
    /// to drive downstream signals like usage-poller activity marking.
    pub fn update_output(&mut self, pty_id: &str, screen_hash: [u8; 32]) -> bool {
        let previous = self.get_status(pty_id);
        let now = Instant::now();
        let entry = self
            .statuses
            .entry(pty_id.to_string())
            .or_insert_with(|| PaneStatusEntry {
                status: PaneStatus::Working,
                last_output_hash: [0u8; 32],
                last_output_time: now,
                last_byte_time: now,
                last_check_time: now,
            });

        // Byte clock resets on every chunk regardless of content change.
        entry.last_byte_time = now;

        let changed = if screen_hash != entry.last_output_hash {
            entry.last_output_hash = screen_hash;
            entry.last_output_time = now;
            entry.status = PaneStatus::Working;
            true
        } else {
            false
        };
        let current = self.get_status(pty_id);
        self.notify_if_changed(pty_id, previous, current);
        changed
    }

    pub fn set_waiting(&mut self, pty_id: &str) {
        let previous = self.get_status(pty_id);
        if let Some(entry) = self.statuses.get_mut(pty_id) {
            entry.status = PaneStatus::Waiting;
        }
        self.notify_if_changed(pty_id, previous, self.get_status(pty_id));
    }

    pub fn set_idle(&mut self, pty_id: &str) {
        let previous = self.get_status(pty_id);
        if let Some(entry) = self.statuses.get_mut(pty_id) {
            entry.status = PaneStatus::Idle;
        }
        self.notify_if_changed(pty_id, previous, self.get_status(pty_id));
    }

    pub fn set_dead(&mut self, pty_id: &str) {
        let previous = self.get_status(pty_id);
        if let Some(entry) = self.statuses.get_mut(pty_id) {
            entry.status = PaneStatus::Dead;
        }
        self.notify_if_changed(pty_id, previous, self.get_status(pty_id));
    }

    pub fn check_idle(&mut self, pty_id: &str, idle_timeout: Duration) {
        let previous = self.get_status(pty_id);
        let now = Instant::now();
        if let Some(entry) = self.statuses.get_mut(pty_id) {
            entry.last_check_time = now;
            if entry.status == PaneStatus::Working
                && now.duration_since(entry.last_output_time) > idle_timeout
            {
                entry.status = PaneStatus::Idle;
            }
        }
        self.notify_if_changed(pty_id, previous, self.get_status(pty_id));
    }

    /// Two-clock quiescence for the live 3-state classifier: `(byte_idle_ms,
    /// semantic_idle_ms)` measured from `now`. `None` if the pty is untracked.
    /// Byte idle grows whenever no output arrives; semantic idle grows whenever
    /// the (status-bar-excluded) content is unchanged — mirrors
    /// `SignalExtractor`\'s `Quiescence` without a second parser.
    pub fn quiescence(&self, pty_id: &str, now: Instant) -> Option<(u64, u64)> {
        self.statuses.get(pty_id).map(|e| {
            (
                now.saturating_duration_since(e.last_byte_time).as_millis() as u64,
                now.saturating_duration_since(e.last_output_time)
                    .as_millis() as u64,
            )
        })
    }

    pub fn get_status(&self, pty_id: &str) -> PaneStatus {
        self.statuses
            .get(pty_id)
            .map(|e| e.status)
            .unwrap_or(PaneStatus::Dead)
    }

    /// Like `get_status`, but `None` for an untracked pty instead of defaulting
    /// to `Dead` — lets snapshot hydration mirror exactly what a live
    /// `pane-status-changed` event would have delivered (no event yet = no rail).
    pub fn get_status_if_tracked(&self, pty_id: &str) -> Option<PaneStatus> {
        self.statuses.get(pty_id).map(|e| e.status)
    }

    pub fn remove(&mut self, pty_id: &str) {
        self.statuses.remove(pty_id);
        if let Some(signal) = self.signals.remove(pty_id) {
            signal.close();
        }
    }

    /// Returns all tracked pty IDs.
    pub fn all_ids(&self) -> Vec<String> {
        self.statuses.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests;
