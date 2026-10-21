use serde::{Deserialize, Serialize};

pub const RETENTION_MS: u64 = 30 * 60 * 1000;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Retention {
    pub transition_offset: usize,
    pub action_offset: usize,
    pub stopped_at: Option<u64>,
    pub deletes_at: Option<u64>,
}

impl Retention {
    pub fn from_stopped_at(stopped_at: Option<u64>) -> Self {
        Self {
            transition_offset: 0,
            action_offset: 0,
            stopped_at,
            deletes_at: stopped_at.map(|at| at.saturating_add(RETENTION_MS)),
        }
    }

    pub fn sync(&mut self, stopped: bool, now: u64) {
        self.stopped_at = stopped.then(|| self.stopped_at.unwrap_or(now));
        self.deletes_at = self.stopped_at.map(|at| at.saturating_add(RETENTION_MS));
    }

    pub fn overdue(&self, stopped: bool, now: u64) -> bool {
        stopped && self.deletes_at.is_some_and(|at| now >= at)
    }
}

#[cfg(test)]
mod tests;
