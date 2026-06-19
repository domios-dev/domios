/// Inter-agent reporting system for orchestration hierarchy.
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::ledger::{LedgerRow, OrchestrationLedger};

/// Worker-declared outcome of a task. Optional — a freeform report (and the
/// synthetic worker-death report) carries no status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReportStatus {
    Done,
    Blocked,
    Partial,
}

impl ReportStatus {
    /// Capitalized label for the deterministic inject block.
    pub fn label(self) -> &'static str {
        match self {
            ReportStatus::Done => "Done",
            ReportStatus::Blocked => "Blocked",
            ReportStatus::Partial => "Partial",
        }
    }
}

/// Optional structured fields a worker can attach to a report. All `None` for a
/// freeform / synthetic message-only report, which then round-trips unchanged.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReportDetails {
    #[serde(default)]
    pub status: Option<ReportStatus>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub sha: Option<String>,
    #[serde(default)]
    pub changed_files: Option<Vec<String>>,
    #[serde(default)]
    pub test_cmd: Option<String>,
    #[serde(default)]
    pub test_result: Option<String>,
    #[serde(default)]
    pub skipped: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentReport {
    pub id: String,
    pub from_pane_id: String,
    pub from_title: String,
    pub message: String,
    pub timestamp: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<ReportStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changed_files: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_cmd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_result: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped: Option<String>,
}

impl AgentReport {
    /// Build the PTY-injected body: a deterministic block of the present
    /// structured fields (stable order) followed by the freeform message. When
    /// no structured field is set, returns the message verbatim — message-only
    /// and synthetic reports inject exactly as they did before this existed.
    pub fn inject_body(&self) -> String {
        let mut lines: Vec<String> = Vec::new();
        if let Some(status) = self.status {
            lines.push(format!("status: {}", status.label()));
        }
        if let Some(ref branch) = self.branch {
            lines.push(format!("branch: {}", branch));
        }
        if let Some(ref sha) = self.sha {
            lines.push(format!("sha: {}", sha));
        }
        match (self.test_cmd.as_deref(), self.test_result.as_deref()) {
            (Some(cmd), Some(result)) => lines.push(format!("tests: {} -> {}", cmd, result)),
            (Some(cmd), None) => lines.push(format!("tests: {}", cmd)),
            (None, Some(result)) => lines.push(format!("tests: -> {}", result)),
            (None, None) => {}
        }
        if let Some(ref changed) = self.changed_files {
            if !changed.is_empty() {
                lines.push(format!("changed: {}", changed.join(", ")));
            }
        }
        if let Some(ref skipped) = self.skipped {
            lines.push(format!("skipped: {}", skipped));
        }
        if lines.is_empty() {
            self.message.clone()
        } else {
            format!("{}\n{}", lines.join("\n"), self.message)
        }
    }
}

/// Stores reports keyed by orchestrator pane ID.
///
/// The in-memory `reports` map is the live notification stream (fast reads,
/// capped, wiped on restart). The optional durable [`OrchestrationLedger`] is
/// the persistent truth: every recorded report also UPSERTs one row per worker
/// so a restarted orchestrator can still see its workers.
pub struct ReportStore {
    reports: HashMap<String, Vec<AgentReport>>,
    max_reports_per_orchestrator: usize,
    ledger: Option<Arc<OrchestrationLedger>>,
}

impl ReportStore {
    pub fn new(max_per_orchestrator: usize) -> Self {
        Self {
            reports: HashMap::new(),
            max_reports_per_orchestrator: max_per_orchestrator,
            ledger: None,
        }
    }

    /// Build a store backed by a durable ledger. Reports recorded through this
    /// store survive daemon restart in the ledger db.
    pub fn with_ledger(max_per_orchestrator: usize, ledger: Arc<OrchestrationLedger>) -> Self {
        Self {
            reports: HashMap::new(),
            max_reports_per_orchestrator: max_per_orchestrator,
            ledger: Some(ledger),
        }
    }

    /// Durable rows for an orchestrator (survive restart). Empty when no ledger
    /// is attached (in-memory test stores) or the orchestrator has no workers.
    pub fn ledger_rows(&self, orchestrator_pane_id: &str) -> Vec<LedgerRow> {
        self.ledger
            .as_ref()
            .and_then(|l| l.list_for_orchestrator(orchestrator_pane_id).ok())
            .unwrap_or_default()
    }

    /// Add a freeform (message-only) report under an orchestrator's pane ID.
    pub fn add(
        &mut self,
        orchestrator_pane_id: &str,
        from_pane_id: &str,
        from_title: &str,
        message: &str,
    ) -> AgentReport {
        self.add_detailed(
            orchestrator_pane_id,
            from_pane_id,
            from_title,
            message,
            ReportDetails::default(),
            None,
        )
    }

    /// Add a report carrying optional structured fields. Message-only callers
    /// pass `ReportDetails::default()` (via [`add`]) for unchanged behaviour.
    /// `model` is the worker's live session model (pane metadata, not part of
    /// the report body) — recorded in the durable ledger only.
    pub fn add_detailed(
        &mut self,
        orchestrator_pane_id: &str,
        from_pane_id: &str,
        from_title: &str,
        message: &str,
        details: ReportDetails,
        model: Option<&str>,
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
            status: details.status,
            branch: details.branch.clone(),
            sha: details.sha.clone(),
            changed_files: details.changed_files.clone(),
            test_cmd: details.test_cmd.clone(),
            test_result: details.test_result.clone(),
            skipped: details.skipped.clone(),
        };

        // Durable truth: UPSERT one row per (orchestrator, worker). Best-effort —
        // a ledger write failure is logged, never blocks the live report path.
        if let Some(ledger) = &self.ledger {
            let row = LedgerRow {
                orchestrator_pane_id: orchestrator_pane_id.to_string(),
                worker_pane_id: from_pane_id.to_string(),
                title: from_title.to_string(),
                branch: details.branch.clone(),
                model: model.map(str::to_string),
                status: details.status,
                message: message.to_string(),
                sha: details.sha.clone(),
                changed_files: details.changed_files.clone(),
                test_cmd: details.test_cmd.clone(),
                test_result: details.test_result.clone(),
                skipped: details.skipped.clone(),
                updated_at: report.timestamp,
            };
            if let Err(e) = ledger.upsert(&row) {
                log::warn!("[ledger] upsert for worker {} failed: {}", from_pane_id, e);
            }
        }

        let entries = self
            .reports
            .entry(orchestrator_pane_id.to_string())
            .or_default();
        entries.push(report.clone());

        // Trim old reports
        if entries.len() > self.max_reports_per_orchestrator {
            let excess = entries.len() - self.max_reports_per_orchestrator;
            entries.drain(..excess);
        }

        report
    }

    /// Get all reports for an orchestrator, optionally clearing them.
    pub fn get(&mut self, orchestrator_pane_id: &str, clear: bool) -> Vec<AgentReport> {
        if clear {
            self.reports
                .remove(orchestrator_pane_id)
                .unwrap_or_default()
        } else {
            self.reports
                .get(orchestrator_pane_id)
                .cloned()
                .unwrap_or_default()
        }
    }

    /// True when at least one report from `from_pane_id` is stored under
    /// `orchestrator_pane_id`. Used by the worker-death watchdog to decide
    /// whether a dying worker has already spoken (so no synthetic report is
    /// needed). There is no separate terminal-report flag — any prior report
    /// counts as the worker having reported up the chain.
    pub fn has_report_from(&self, orchestrator_pane_id: &str, from_pane_id: &str) -> bool {
        self.reports
            .get(orchestrator_pane_id)
            .map(|v| v.iter().any(|r| r.from_pane_id == from_pane_id))
            .unwrap_or(false)
    }

    /// Get report count for an orchestrator.
    pub fn count(&self, orchestrator_pane_id: &str) -> usize {
        self.reports
            .get(orchestrator_pane_id)
            .map(|v| v.len())
            .unwrap_or(0)
    }

    /// Remove all reports for an orchestrator (cleanup on close).
    pub fn remove(&mut self, orchestrator_pane_id: &str) {
        self.reports.remove(orchestrator_pane_id);
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
        store.add("cond-2", "w3", "Worker 3", "Other orchestrator");

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
    fn test_has_report_from() {
        let mut store = ReportStore::new(100);
        assert!(!store.has_report_from("cond-1", "w1"));
        store.add("cond-1", "w1", "W1", "done");
        assert!(store.has_report_from("cond-1", "w1"));
        // Different reporter under the same target is not a match.
        assert!(!store.has_report_from("cond-1", "w2"));
        // Same reporter under a different target is not a match.
        assert!(!store.has_report_from("cond-2", "w1"));
    }

    #[test]
    fn test_remove() {
        let mut store = ReportStore::new(100);
        store.add("cond-1", "w1", "W1", "msg");
        store.remove("cond-1");
        assert_eq!(store.count("cond-1"), 0);
    }

    #[test]
    fn structured_report_round_trips_through_serde() {
        let details = ReportDetails {
            status: Some(ReportStatus::Partial),
            branch: Some("orch-p2-structured-report".to_string()),
            sha: Some("deadbeef".to_string()),
            changed_files: Some(vec!["a.rs".to_string(), "b.rs".to_string()]),
            test_cmd: Some("cargo test".to_string()),
            test_result: Some("ok".to_string()),
            skipped: Some("e2e".to_string()),
        };
        let mut store = ReportStore::new(100);
        let stored = store.add_detailed("orch", "w1", "Worker", "all done", details, None);

        // Through the store, the typed fields survive.
        let listed = store.get("orch", false);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].status, Some(ReportStatus::Partial));
        assert_eq!(listed[0].changed_files.as_deref().unwrap().len(), 2);

        // Through serde JSON, every field round-trips byte-for-byte.
        let json = serde_json::to_string(&stored).unwrap();
        let back: AgentReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.status, Some(ReportStatus::Partial));
        assert_eq!(back.branch.as_deref(), Some("orch-p2-structured-report"));
        assert_eq!(back.sha.as_deref(), Some("deadbeef"));
        assert_eq!(
            back.changed_files,
            Some(vec!["a.rs".to_string(), "b.rs".to_string()])
        );
        assert_eq!(back.test_cmd.as_deref(), Some("cargo test"));
        assert_eq!(back.test_result.as_deref(), Some("ok"));
        assert_eq!(back.skipped.as_deref(), Some("e2e"));
    }

    #[test]
    fn message_only_report_omits_structured_keys_in_json() {
        // Synthetic / freeform reports (P1 worker-death watchdog) must serialize
        // without any of the new keys so old consumers round-trip unchanged.
        let mut store = ReportStore::new(100);
        let report = store.add(
            "orch",
            "w1",
            "Worker",
            "WORKER w1 EXITED without reporting (dead)",
        );
        let json = serde_json::to_value(&report).unwrap();
        for key in [
            "status",
            "branch",
            "sha",
            "changed_files",
            "test_cmd",
            "test_result",
            "skipped",
        ] {
            assert!(
                json.get(key).is_none(),
                "message-only report must not serialize key {key}"
            );
        }
    }

    #[test]
    fn message_only_report_injects_just_the_message() {
        // No structured fields → inject text is exactly the freeform message,
        // proving no regression for P1's synthetic message-only reports.
        let mut store = ReportStore::new(100);
        let report = store.add("orch", "w1", "Worker", "just the body");
        assert_eq!(report.inject_body(), "just the body");
    }

    #[test]
    fn structured_inject_block_renders_in_stable_order() {
        let details = ReportDetails {
            status: Some(ReportStatus::Done),
            branch: Some("feature-x".to_string()),
            sha: Some("abc123".to_string()),
            changed_files: Some(vec!["src/a.rs".to_string(), "src/b.rs".to_string()]),
            test_cmd: Some("cargo test".to_string()),
            test_result: Some("42 passed".to_string()),
            skipped: Some("manual UI step".to_string()),
        };
        let mut store = ReportStore::new(100);
        let report = store.add_detailed("orch", "w1", "Worker", "the freeform body", details, None);
        let expected = "status: Done\n\
             branch: feature-x\n\
             sha: abc123\n\
             tests: cargo test -> 42 passed\n\
             changed: src/a.rs, src/b.rs\n\
             skipped: manual UI step\n\
             the freeform body";
        assert_eq!(report.inject_body(), expected);
    }

    #[test]
    fn report_status_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&ReportStatus::Done).unwrap(),
            "\"done\""
        );
        assert_eq!(
            serde_json::from_str::<ReportStatus>("\"blocked\"").unwrap(),
            ReportStatus::Blocked
        );
    }

    #[test]
    fn ledger_backed_report_persists_latest_status_across_reopen() {
        use crate::agent::ledger::OrchestrationLedger;

        let db = std::env::temp_dir().join(format!(
            "reports-ledger-{}/orchestration.db",
            ulid::Ulid::new()
        ));
        {
            let ledger = std::sync::Arc::new(OrchestrationLedger::open_at(&db).unwrap());
            let mut store = ReportStore::with_ledger(100, ledger);
            // Same worker reports twice with changing status + model.
            let mut d = ReportDetails::default();
            d.status = Some(ReportStatus::Partial);
            d.branch = Some("orch-p3-ledger".into());
            store.add_detailed("orch", "w1", "Worker", "wip", d.clone(), Some("opus"));
            d.status = Some(ReportStatus::Done);
            store.add_detailed("orch", "w1", "Worker", "done", d, Some("opus"));
        } // store + ledger Connection dropped — simulates restart.

        // Reopen the same db: the durable ledger still has exactly one row,
        // latest status.
        let reopened = OrchestrationLedger::open_at(&db).unwrap();
        let rows = reopened.list_for_orchestrator("orch").unwrap();
        assert_eq!(rows.len(), 1, "one row per worker after re-report");
        assert_eq!(rows[0].status, Some(ReportStatus::Done));
        assert_eq!(rows[0].model.as_deref(), Some("opus"));
        assert_eq!(rows[0].branch.as_deref(), Some("orch-p3-ledger"));
        std::fs::remove_dir_all(db.parent().unwrap()).ok();
    }

    #[test]
    fn test_separate_orchestrators() {
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
