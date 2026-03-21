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
