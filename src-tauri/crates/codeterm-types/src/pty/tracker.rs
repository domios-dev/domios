use std::collections::HashMap;
use std::time::{Duration, Instant};

use super::pane_status::PaneStatus;

pub struct PaneStatusTracker {
    statuses: HashMap<String, PaneStatusEntry>,
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
        }
    }

    /// Update status based on new PTY output. Returns `true` when the screen
    /// content actually changed (i.e. real activity, not a cursor blink or
    /// TUI repaint with identical content). Callers can use the return value
    /// to drive downstream signals like usage-poller activity marking.
    pub fn update_output(&mut self, pty_id: &str, screen_hash: [u8; 32]) -> bool {
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

        if screen_hash != entry.last_output_hash {
            entry.last_output_hash = screen_hash;
            entry.last_output_time = now;
            entry.status = PaneStatus::Working;
            true
        } else {
            false
        }
    }

    pub fn set_waiting(&mut self, pty_id: &str) {
        if let Some(entry) = self.statuses.get_mut(pty_id) {
            entry.status = PaneStatus::Waiting;
        }
    }

    pub fn set_idle(&mut self, pty_id: &str) {
        if let Some(entry) = self.statuses.get_mut(pty_id) {
            entry.status = PaneStatus::Idle;
        }
    }

    pub fn set_dead(&mut self, pty_id: &str) {
        if let Some(entry) = self.statuses.get_mut(pty_id) {
            entry.status = PaneStatus::Dead;
        }
    }

    pub fn check_idle(&mut self, pty_id: &str, idle_timeout: Duration) {
        let now = Instant::now();
        if let Some(entry) = self.statuses.get_mut(pty_id) {
            entry.last_check_time = now;
            if entry.status == PaneStatus::Working
                && now.duration_since(entry.last_output_time) > idle_timeout
            {
                entry.status = PaneStatus::Idle;
            }
        }
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
    }

    /// Returns all tracked pty IDs.
    pub fn all_ids(&self) -> Vec<String> {
        self.statuses.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests;
