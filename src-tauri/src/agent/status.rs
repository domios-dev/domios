/// Per-PTY agent status tracking.
/// Keyed by pty_id — use mux.pane_id_for_pty() to translate.
use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum PaneStatus {
    Working,
    Waiting,
    Idle,
    Dead,
}

pub struct PaneStatusTracker {
    statuses: HashMap<String, PaneStatusEntry>,
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
                last_check_time: now,
            });

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

    pub fn get_status(&self, pty_id: &str) -> PaneStatus {
        self.statuses
            .get(pty_id)
            .map(|e| e.status)
            .unwrap_or(PaneStatus::Dead)
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
    fn test_update_output_returns_true_on_content_change() {
        let mut tracker = PaneStatusTracker::new();
        assert!(tracker.update_output("pty-1", [1u8; 32]));
        assert!(tracker.update_output("pty-1", [2u8; 32]));
    }

    #[test]
    fn test_update_output_returns_false_on_same_hash() {
        let mut tracker = PaneStatusTracker::new();
        tracker.update_output("pty-1", [1u8; 32]);
        assert!(!tracker.update_output("pty-1", [1u8; 32]));
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
