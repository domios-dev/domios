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

    pub fn update_output(&mut self, pane_id: &str, data: &[u8]) {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let hash: [u8; 32] = hasher.finalize().into();
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

        entry.last_output_hash = hash;
        entry.last_output_time = now;
        entry.status = PaneStatus::Working;
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
}
