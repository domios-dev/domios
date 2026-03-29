/// Per-pane agent status tracking.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum PaneStatus {
    Working,
    Waiting,
    Idle,
    Dead,
}

pub struct PaneStatusTracker {
    pane_statuses: HashMap<String, PaneStatusEntry>,
}

struct PaneStatusEntry {
    status: PaneStatus,
    last_output_hash: [u8; 32],
    last_output_time: Instant,
    #[allow(dead_code)]
    last_check_time: Instant,
}

impl PaneStatusTracker {
    pub fn new() -> Self {
        Self {
            pane_statuses: HashMap::new(),
        }
    }

    /// Update status based on new PTY output.
    /// `screen_hash` is the hash of the current screen content (what user sees),
    /// NOT the raw PTY bytes. This filters out cursor blinks and TUI noise —
    /// only actual content changes count as "working".
    pub fn update_output(&mut self, pane_id: &str, screen_hash: [u8; 32]) {
        let now = Instant::now();

        let entry = self
            .pane_statuses
            .entry(pane_id.to_string())
            .or_insert_with(|| PaneStatusEntry {
                status: PaneStatus::Working,
                last_output_hash: [0u8; 32],
                last_output_time: now,
                last_check_time: now,
            });

        // Only count as activity if screen content actually changed
        if screen_hash != entry.last_output_hash {
            entry.last_output_hash = screen_hash;
            entry.last_output_time = now;
            entry.status = PaneStatus::Working;
        }
    }

    pub fn set_waiting(&mut self, pane_id: &str) {
        if let Some(entry) = self.pane_statuses.get_mut(pane_id) {
            entry.status = PaneStatus::Waiting;
        }
    }

    pub fn set_dead(&mut self, pane_id: &str) {
        if let Some(entry) = self.pane_statuses.get_mut(pane_id) {
            entry.status = PaneStatus::Dead;
        }
    }

    pub fn check_idle(&mut self, pane_id: &str, idle_timeout: Duration) {
        let now = Instant::now();
        if let Some(entry) = self.pane_statuses.get_mut(pane_id) {
            entry.last_check_time = now;
            if entry.status == PaneStatus::Working
                && now.duration_since(entry.last_output_time) > idle_timeout
            {
                entry.status = PaneStatus::Idle;
            }
        }
    }

    pub fn get_status(&self, pane_id: &str) -> PaneStatus {
        self.pane_statuses
            .get(pane_id)
            .map(|e| e.status)
            .unwrap_or(PaneStatus::Dead)
    }

    pub fn remove(&mut self, pane_id: &str) {
        self.pane_statuses.remove(pane_id);
    }

    /// Returns all tracked pane/pty IDs.
    pub fn all_ids(&self) -> Vec<String> {
        self.pane_statuses.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unknown_pane_is_dead() {
        let tracker = PaneStatusTracker::new();
        assert_eq!(tracker.get_status("nonexistent"), PaneStatus::Dead);
    }

    #[test]
    fn test_update_output_sets_working() {
        let mut tracker = PaneStatusTracker::new();
        tracker.update_output("pty-1", [1u8; 32]);
        assert_eq!(tracker.get_status("pty-1"), PaneStatus::Working);
    }

    #[test]
    fn test_set_waiting() {
        let mut tracker = PaneStatusTracker::new();
        tracker.update_output("pty-1", [1u8; 32]);
        tracker.set_waiting("pty-1");
        assert_eq!(tracker.get_status("pty-1"), PaneStatus::Waiting);
    }

    #[test]
    fn test_set_dead() {
        let mut tracker = PaneStatusTracker::new();
        tracker.update_output("pty-1", [1u8; 32]);
        tracker.set_dead("pty-1");
        assert_eq!(tracker.get_status("pty-1"), PaneStatus::Dead);
    }

    #[test]
    fn test_check_idle_not_timed_out() {
        let mut tracker = PaneStatusTracker::new();
        tracker.update_output("pty-1", [1u8; 32]);
        tracker.check_idle("pty-1", Duration::from_secs(60));
        // Should still be working (not enough time passed)
        assert_eq!(tracker.get_status("pty-1"), PaneStatus::Working);
    }

    #[test]
    fn test_check_idle_timed_out() {
        let mut tracker = PaneStatusTracker::new();
        tracker.update_output("pty-1", [1u8; 32]);
        // Use zero duration to trigger immediate idle
        tracker.check_idle("pty-1", Duration::from_millis(0));
        assert_eq!(tracker.get_status("pty-1"), PaneStatus::Idle);
    }

    #[test]
    fn test_update_output_resets_to_working() {
        let mut tracker = PaneStatusTracker::new();
        tracker.update_output("pty-1", [1u8; 32]);
        tracker.check_idle("pty-1", Duration::from_millis(0));
        assert_eq!(tracker.get_status("pty-1"), PaneStatus::Idle);

        // New output should reset to Working
        tracker.update_output("pty-1", [2u8; 32]);
        assert_eq!(tracker.get_status("pty-1"), PaneStatus::Working);
    }

    #[test]
    fn test_remove() {
        let mut tracker = PaneStatusTracker::new();
        tracker.update_output("pty-1", [1u8; 32]);
        tracker.remove("pty-1");
        assert_eq!(tracker.get_status("pty-1"), PaneStatus::Dead);
    }

    #[test]
    fn test_same_screen_hash_does_not_reset_timer() {
        let mut tracker = PaneStatusTracker::new();
        tracker.update_output("pty-1", [1u8; 32]);
        // Same hash — should NOT reset idle timer or set Working again
        tracker.check_idle("pty-1", Duration::from_millis(0));
        assert_eq!(tracker.get_status("pty-1"), PaneStatus::Idle);
        // Same hash after idle — stays idle
        tracker.update_output("pty-1", [1u8; 32]);
        assert_eq!(tracker.get_status("pty-1"), PaneStatus::Idle);
    }

    #[test]
    fn test_all_ids() {
        let mut tracker = PaneStatusTracker::new();
        tracker.update_output("pty-1", [1u8; 32]);
        tracker.update_output("pty-2", [2u8; 32]);
        let ids = tracker.all_ids();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"pty-1".to_string()));
        assert!(ids.contains(&"pty-2".to_string()));
    }

    #[test]
    fn test_waiting_not_affected_by_idle_check() {
        let mut tracker = PaneStatusTracker::new();
        tracker.update_output("pty-1", [1u8; 32]);
        tracker.set_waiting("pty-1");
        // check_idle should not affect Waiting status (only transitions Working→Idle)
        tracker.check_idle("pty-1", Duration::from_millis(0));
        assert_eq!(tracker.get_status("pty-1"), PaneStatus::Waiting);
    }
}
