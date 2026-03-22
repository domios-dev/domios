/// Inter-agent reporting system for orchestration hierarchy.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct AgentReport {
    pub id: String,
    pub from_pane_id: String,
    pub from_title: String,
    pub message: String,
    pub timestamp: u64,
}

/// Stores reports keyed by conductor pane ID.
pub struct ReportStore {
    reports: HashMap<String, Vec<AgentReport>>,
    max_reports_per_conductor: usize,
}

impl ReportStore {
    pub fn new(max_per_conductor: usize) -> Self {
        Self {
            reports: HashMap::new(),
            max_reports_per_conductor: max_per_conductor,
        }
    }

    /// Add a report under a conductor's pane ID.
    pub fn add(
        &mut self,
        conductor_pane_id: &str,
        from_pane_id: &str,
        from_title: &str,
        message: &str,
    ) -> AgentReport {
        let report = AgentReport {
            id: uuid::Uuid::new_v4().to_string(),
            from_pane_id: from_pane_id.to_string(),
            from_title: from_title.to_string(),
            message: message.to_string(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };

        let entries = self
            .reports
            .entry(conductor_pane_id.to_string())
            .or_default();
        entries.push(report.clone());

        // Trim old reports
        if entries.len() > self.max_reports_per_conductor {
            let excess = entries.len() - self.max_reports_per_conductor;
            entries.drain(..excess);
        }

        report
    }

    /// Get all reports for a conductor, optionally clearing them.
    pub fn get(&mut self, conductor_pane_id: &str, clear: bool) -> Vec<AgentReport> {
        if clear {
            self.reports
                .remove(conductor_pane_id)
                .unwrap_or_default()
        } else {
            self.reports
                .get(conductor_pane_id)
                .cloned()
                .unwrap_or_default()
        }
    }

    /// Get report count for a conductor.
    pub fn count(&self, conductor_pane_id: &str) -> usize {
        self.reports
            .get(conductor_pane_id)
            .map(|v| v.len())
            .unwrap_or(0)
    }

    /// Remove all reports for a conductor (cleanup on close).
    pub fn remove(&mut self, conductor_pane_id: &str) {
        self.reports.remove(conductor_pane_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_report() {
        let mut store = ReportStore::new(100);
        let report = store.add("cond-1", "worker-1", "Worker 1", "Task complete");
        assert!(!report.id.is_empty());
        assert_eq!(report.from_pane_id, "worker-1");
        assert_eq!(report.from_title, "Worker 1");
        assert_eq!(report.message, "Task complete");
    }

    #[test]
    fn test_get_reports() {
        let mut store = ReportStore::new(100);
        store.add("cond-1", "w1", "Worker 1", "Done");
        store.add("cond-1", "w2", "Worker 2", "Done too");
        store.add("cond-2", "w3", "Worker 3", "Other conductor");

        let reports = store.get("cond-1", false);
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].from_pane_id, "w1");
        assert_eq!(reports[1].from_pane_id, "w2");
    }

    #[test]
    fn test_get_reports_clear() {
        let mut store = ReportStore::new(100);
        store.add("cond-1", "w1", "Worker", "Done");
        let reports = store.get("cond-1", true);
        assert_eq!(reports.len(), 1);
        // Should be empty now
        let reports2 = store.get("cond-1", false);
        assert_eq!(reports2.len(), 0);
    }

    #[test]
    fn test_get_reports_empty() {
        let mut store = ReportStore::new(100);
        let reports = store.get("nonexistent", false);
        assert!(reports.is_empty());
    }

    #[test]
    fn test_count() {
        let mut store = ReportStore::new(100);
        assert_eq!(store.count("cond-1"), 0);
        store.add("cond-1", "w1", "W1", "msg");
        assert_eq!(store.count("cond-1"), 1);
        store.add("cond-1", "w2", "W2", "msg");
        assert_eq!(store.count("cond-1"), 2);
    }

    #[test]
    fn test_eviction() {
        let mut store = ReportStore::new(3);
        store.add("cond-1", "w1", "W1", "A");
        store.add("cond-1", "w2", "W2", "B");
        store.add("cond-1", "w3", "W3", "C");
        store.add("cond-1", "w4", "W4", "D");
        assert_eq!(store.count("cond-1"), 3);
        let reports = store.get("cond-1", false);
        assert_eq!(reports[0].message, "B"); // A was evicted
    }

    #[test]
    fn test_remove() {
        let mut store = ReportStore::new(100);
        store.add("cond-1", "w1", "W1", "msg");
        store.remove("cond-1");
        assert_eq!(store.count("cond-1"), 0);
    }

    #[test]
    fn test_separate_conductors() {
        let mut store = ReportStore::new(100);
        store.add("cond-1", "w1", "W1", "msg1");
        store.add("cond-2", "w2", "W2", "msg2");
        assert_eq!(store.count("cond-1"), 1);
        assert_eq!(store.count("cond-2"), 1);
        store.remove("cond-1");
        assert_eq!(store.count("cond-1"), 0);
        assert_eq!(store.count("cond-2"), 1);
    }
}
