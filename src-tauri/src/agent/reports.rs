/// Inter-agent reporting system for orchestration hierarchy.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::ledger::{LedgerRow, OrchestrationLedger};

pub use codeterm_types::agent::ReportStatus;

/// Optional structured fields a worker can attach to a report. All `None` for a
/// freeform / synthetic message-only report, which then round-trips unchanged.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReportDetails {
    #[serde(default)]
    pub plan_id: Option<String>,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub owner_epoch: Option<u64>,
    #[serde(default)]
    pub caller_tab_id: Option<String>,
    #[serde(default)]
    pub caller_session_id: Option<String>,
    #[serde(default)]
    pub session_binding_nonce: Option<String>,
    #[serde(default)]
    pub attempt_id: Option<String>,
    #[serde(default)]
    pub status: Option<ReportStatus>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub base: Option<String>,
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
    #[serde(default)]
    pub attachments: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentReport {
    pub id: String,
    pub from_tab_id: String,
    pub from_title: String,
    pub message: String,
    pub timestamp: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<ReportStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changed_files: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_cmd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_result: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_outcome: Option<codeterm_types::agent::ReportOutcomeClass>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<crate::plan::AttachmentRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planned_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_resolved_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_fallback: Option<crate::multiplexer::types::ModelFallbackNotice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_identity_mismatch: Option<crate::multiplexer::types::ModelIdentityMismatch>,
    #[serde(
        default,
        skip_serializing_if = "crate::multiplexer::types::ReasoningEffortIdentity::is_empty"
    )]
    pub reasoning_effort: crate::multiplexer::types::ReasoningEffortIdentity,
}

impl AgentReport {
    /// Build the PTY-injected body: a deterministic block of the present
    /// structured fields (stable order) followed by the freeform message. When
    /// no structured field is set, returns the message verbatim — message-only
    /// and synthetic reports inject exactly as they did before this existed.
    pub fn inject_body(&self) -> String {
        let mut lines: Vec<String> = Vec::new();
        match (self.plan_id.as_deref(), self.task_id.as_deref()) {
            (Some(plan), Some(task)) => lines.push(format!("task: {plan}/{task}")),
            (Some(plan), None) => lines.push(format!("plan: {plan}")),
            (None, Some(task)) => lines.push(format!("task: {task}")),
            (None, None) => {}
        }
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

/// Stores reports keyed by orchestrator tab ID.
///
/// The in-memory `reports` map is the live notification stream (fast reads,
/// capped, wiped on restart). The optional durable [`OrchestrationLedger`] is
/// the persistent truth: every recorded report also UPSERTs one row per worker
/// so a restarted orchestrator can still see its workers.
pub struct ReportStore {
    reports: HashMap<String, Vec<AgentReport>>,
    delivered: HashSet<String>,
    in_flight: HashSet<String>,
    max_reports_per_orchestrator: usize,
    ledger: Option<Arc<OrchestrationLedger>>,
}

impl ReportStore {
    pub fn new(max_per_orchestrator: usize) -> Self {
        Self {
            reports: HashMap::new(),
            delivered: HashSet::new(),
            in_flight: HashSet::new(),
            max_reports_per_orchestrator: max_per_orchestrator,
            ledger: None,
        }
    }

    /// Build a store backed by a durable ledger. Reports recorded through this
    /// store survive daemon restart in the ledger db.
    pub fn with_ledger(max_per_orchestrator: usize, ledger: Arc<OrchestrationLedger>) -> Self {
        Self {
            reports: HashMap::new(),
            delivered: HashSet::new(),
            in_flight: HashSet::new(),
            max_reports_per_orchestrator: max_per_orchestrator,
            ledger: Some(ledger),
        }
    }

    /// Durable rows for an orchestrator (survive restart). Empty when no ledger
    /// is attached (in-memory test stores) or the orchestrator has no workers.
    pub fn ledger_rows(&self, orchestrator_tab_id: &str) -> Vec<LedgerRow> {
        self.ledger
            .as_ref()
            .and_then(|l| l.list_for_orchestrator(orchestrator_tab_id).ok())
            .unwrap_or_default()
    }

    /// Durable audit row for a spawn that carries no worker report yet. Gives a
    /// roleless spawn a trackable trail (parent + ledger entry) so an operator
    /// can corroborate it instead of trusting a spoofable self-label.
    pub fn record_spawn_audit(&self, parent_tab_id: &str, tab_id: &str, title: &str, now: u64) {
        let Some(ledger) = self.ledger.as_ref() else {
            return;
        };
        let row = LedgerRow {
            orchestrator_tab_id: parent_tab_id.to_string(),
            worker_tab_id: tab_id.to_string(),
            title: title.to_string(),
            message: "roleless spawn (tracked)".to_string(),
            updated_at: now,
            ..Default::default()
        };
        if let Err(e) = ledger.upsert(&row) {
            log::warn!("[ledger] spawn audit upsert failed: {}", e);
        }
    }

    /// Completed lanes across all orchestrators for the durable Done surface,
    /// most-recently-finished first. Empty without a ledger.
    pub fn completed_rows(&self, limit: usize) -> Vec<LedgerRow> {
        self.ledger
            .as_ref()
            .and_then(|l| l.list_completed(limit).ok())
            .unwrap_or_default()
    }

    /// Add a freeform (message-only) report under an orchestrator's tab ID.
    pub fn add(
        &mut self,
        orchestrator_tab_id: &str,
        from_tab_id: &str,
        from_title: &str,
        message: &str,
    ) -> AgentReport {
        self.add_detailed(
            orchestrator_tab_id,
            from_tab_id,
            from_title,
            message,
            ReportDetails::default(),
            None,
        )
    }

    /// Add a report carrying optional structured fields. Message-only callers
    /// pass `ReportDetails::default()` (via [`add`]) for unchanged behaviour.
    /// `model` is the worker's live session model (tab metadata, not part of
    /// the report body) — recorded in the durable ledger only.
    pub fn add_detailed(
        &mut self,
        orchestrator_tab_id: &str,
        from_tab_id: &str,
        from_title: &str,
        message: &str,
        details: ReportDetails,
        model: Option<&str>,
    ) -> AgentReport {
        self.add_detailed_with_model_identity(
            orchestrator_tab_id,
            from_tab_id,
            from_title,
            message,
            details,
            model.map(|m| ReportModelIdentity {
                actual_model: Some(m.to_string()),
                planned_model: None,
                requested_model: None,
                provider_resolved_model: None,
                fallback: None,
                mismatch: None,
                reasoning_effort: Default::default(),
            }),
        )
    }

    pub fn add_detailed_with_model_identity(
        &mut self,
        orchestrator_tab_id: &str,
        from_tab_id: &str,
        from_title: &str,
        message: &str,
        details: ReportDetails,
        model_identity: Option<ReportModelIdentity>,
    ) -> AgentReport {
        self.add_detailed_with_model_identity_and_outcome(
            orchestrator_tab_id,
            from_tab_id,
            from_title,
            message,
            details,
            model_identity,
            None,
        )
    }

    pub fn add_detailed_with_model_identity_and_outcome(
        &mut self,
        orchestrator_tab_id: &str,
        from_tab_id: &str,
        from_title: &str,
        message: &str,
        details: ReportDetails,
        model_identity: Option<ReportModelIdentity>,
        report_outcome: Option<codeterm_types::agent::ReportOutcomeClass>,
    ) -> AgentReport {
        let actual_model = model_identity
            .as_ref()
            .and_then(|identity| identity.actual_model.clone());
        let planned_model = model_identity
            .as_ref()
            .and_then(|identity| identity.planned_model.clone());
        let requested_model = model_identity
            .as_ref()
            .and_then(|identity| identity.requested_model.clone());
        let provider_resolved_model = model_identity
            .as_ref()
            .and_then(|identity| identity.provider_resolved_model.clone());
        let fallback = model_identity
            .as_ref()
            .and_then(|identity| identity.fallback.clone());
        let mismatch = model_identity
            .as_ref()
            .and_then(|identity| identity.mismatch.clone());
        let reasoning_effort = model_identity
            .as_ref()
            .map(|identity| identity.reasoning_effort.clone())
            .unwrap_or_default();
        let report = AgentReport {
            id: uuid::Uuid::new_v4().to_string(),
            from_tab_id: from_tab_id.to_string(),
            from_title: from_title.to_string(),
            message: message.to_string(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            plan_id: details.plan_id.clone(),
            task_id: details.task_id.clone(),
            status: details.status,
            branch: details.branch.clone(),
            sha: details.sha.clone(),
            attempt_id: details.attempt_id.clone(),
            base: details.base.clone(),
            changed_files: details.changed_files.clone(),
            test_cmd: details.test_cmd.clone(),
            test_result: details.test_result.clone(),
            skipped: details.skipped.clone(),
            report_outcome,
            attachments: Vec::new(),
            model: actual_model.clone(),
            planned_model,
            requested_model,
            provider_resolved_model,
            actual_model: actual_model.clone(),
            model_fallback: fallback,
            model_identity_mismatch: mismatch,
            reasoning_effort,
        };

        // a ledger write failure is logged, never blocks the live report path.
        if let Some(ledger) = &self.ledger {
            let row = LedgerRow {
                orchestrator_tab_id: orchestrator_tab_id.to_string(),
                worker_tab_id: from_tab_id.to_string(),
                title: from_title.to_string(),
                plan_id: details.plan_id.clone(),
                task_id: details.task_id.clone(),
                branch: details.branch.clone(),
                model: actual_model,
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
                log::warn!("[ledger] upsert for worker {} failed: {}", from_tab_id, e);
            }
        }

        {
            let entries = self
                .reports
                .entry(orchestrator_tab_id.to_string())
                .or_default();
            entries.push(report.clone());
            if entries.len() > self.max_reports_per_orchestrator {
                let excess = entries.len() - self.max_reports_per_orchestrator;
                entries.drain(..excess);
            }
        }
        let live_ids: HashSet<&str> = self
            .reports
            .values()
            .flatten()
            .map(|entry| entry.id.as_str())
            .collect();
        self.delivered.retain(|id| live_ids.contains(id.as_str()));
        self.in_flight.retain(|id| live_ids.contains(id.as_str()));

        report
    }

    /// Get all reports for an orchestrator, optionally clearing them.
    pub fn get(&mut self, orchestrator_tab_id: &str, clear: bool) -> Vec<AgentReport> {
        if clear {
            let removed = self.reports.remove(orchestrator_tab_id).unwrap_or_default();
            for report in &removed {
                self.delivered.remove(&report.id);
                self.in_flight.remove(&report.id);
            }
            removed
        } else {
            self.reports
                .get(orchestrator_tab_id)
                .cloned()
                .unwrap_or_default()
        }
    }

    pub fn pending_for(&self, orchestrator_tab_id: &str) -> Vec<AgentReport> {
        self.reports
            .get(orchestrator_tab_id)
            .into_iter()
            .flatten()
            .filter(|report| {
                !self.delivered.contains(&report.id) && !self.in_flight.contains(&report.id)
            })
            .cloned()
            .collect()
    }

    pub fn claim_pending_for(&mut self, orchestrator_tab_id: &str) -> Vec<AgentReport> {
        let pending = self.pending_for(orchestrator_tab_id);
        self.in_flight
            .extend(pending.iter().map(|report| report.id.clone()));
        pending
    }

    pub fn claim(&mut self, report_id: &str) -> bool {
        if self.delivered.contains(report_id) || self.in_flight.contains(report_id) {
            return false;
        }
        self.in_flight.insert(report_id.to_string())
    }

    pub fn mark_delivered(&mut self, report_id: &str) {
        self.in_flight.remove(report_id);
        self.delivered.insert(report_id.to_string());
    }

    pub fn release_delivery(&mut self, report_id: &str) {
        self.in_flight.remove(report_id);
    }

    /// True when at least one report from `from_tab_id` is stored under
    /// `orchestrator_tab_id`. Used by the worker-death watchdog to decide
    /// whether a dying worker has already spoken (so no synthetic report is
    /// needed). There is no separate terminal-report flag — any prior report
    /// counts as the worker having reported up the chain.
    pub fn has_report_from(&self, orchestrator_tab_id: &str, from_tab_id: &str) -> bool {
        self.reports
            .get(orchestrator_tab_id)
            .map(|v| v.iter().any(|r| r.from_tab_id == from_tab_id))
            .unwrap_or(false)
    }

    pub fn has_report_from_after(
        &self,
        orchestrator_tab_id: &str,
        from_tab_id: &str,
        timestamp_secs: u64,
    ) -> bool {
        self.reports
            .get(orchestrator_tab_id)
            .is_some_and(|reports| {
                reports.iter().any(|report| {
                    report.from_tab_id == from_tab_id && report.timestamp >= timestamp_secs
                })
            })
    }

    /// Get report count for an orchestrator.
    pub fn count(&self, orchestrator_tab_id: &str) -> usize {
        self.reports
            .get(orchestrator_tab_id)
            .map(|v| v.len())
            .unwrap_or(0)
    }

    /// Remove all reports for an orchestrator (cleanup on close).
    pub fn remove(&mut self, orchestrator_tab_id: &str) {
        if let Some(removed) = self.reports.remove(orchestrator_tab_id) {
            for report in removed {
                self.delivered.remove(&report.id);
                self.in_flight.remove(&report.id);
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ReportModelIdentity {
    pub actual_model: Option<String>,
    pub planned_model: Option<String>,
    pub requested_model: Option<String>,
    pub provider_resolved_model: Option<String>,
    pub fallback: Option<crate::multiplexer::types::ModelFallbackNotice>,
    pub mismatch: Option<crate::multiplexer::types::ModelIdentityMismatch>,
    pub reasoning_effort: crate::multiplexer::types::ReasoningEffortIdentity,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_report() {
        let mut store = ReportStore::new(100);
        let report = store.add("cond-1", "worker-1", "Worker 1", "Task complete");
        assert!(!report.id.is_empty());
        assert_eq!(report.from_tab_id, "worker-1");
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
        assert_eq!(reports[0].from_tab_id, "w1");
        assert_eq!(reports[1].from_tab_id, "w2");
    }

    #[test]
    fn test_get_reports_clear() {
        let mut store = ReportStore::new(100);
        store.add("cond-1", "w1", "Worker", "Done");
        let reports = store.get("cond-1", true);
        assert_eq!(reports.len(), 1);
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
    fn pending_reports_clear_only_after_delivery() {
        let mut store = ReportStore::new(100);
        let report = store.add("cond-1", "w1", "W1", "ready");
        assert_eq!(store.pending_for("cond-1").len(), 1);
        assert_eq!(store.claim_pending_for("cond-1").len(), 1);
        assert!(store.pending_for("cond-1").is_empty());
        assert!(store.claim_pending_for("cond-1").is_empty());
        store.release_delivery(&report.id);
        assert_eq!(store.pending_for("cond-1").len(), 1);
        assert_eq!(store.claim_pending_for("cond-1").len(), 1);
        store.mark_delivered(&report.id);
        assert!(store.pending_for("cond-1").is_empty());
        assert!(store.claim_pending_for("cond-1").is_empty());
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
    fn has_report_from_after_uses_the_worker_turn_boundary() {
        let mut store = ReportStore::new(100);
        let before_report = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .saturating_sub(1);
        store.add("cond-1", "w1", "W1", "done");
        assert!(store.has_report_from_after("cond-1", "w1", before_report));

        let after_report = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .saturating_add(30);
        assert!(!store.has_report_from_after("cond-1", "w1", after_report));
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
            plan_id: Some("pl_structured".to_string()),
            task_id: Some("task_structured".to_string()),
            owner_epoch: Some(2),
            caller_tab_id: Some("w1".into()),
            caller_session_id: Some("session".into()),
            session_binding_nonce: Some("nonce".into()),
            attempt_id: None,
            status: Some(ReportStatus::Partial),
            branch: Some("orch-p2-structured-report".to_string()),
            base: None,
            sha: Some("deadbeef".to_string()),
            changed_files: Some(vec!["a.rs".to_string(), "b.rs".to_string()]),
            test_cmd: Some("cargo test".to_string()),
            test_result: Some("ok".to_string()),
            skipped: Some("e2e".to_string()),
            attachments: vec![],
        };
        let mut store = ReportStore::new(100);
        let stored = store.add_detailed("orch", "w1", "Worker", "all done", details, None);

        // Through the store, the typed fields survive.
        let listed = store.get("orch", false);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].plan_id.as_deref(), Some("pl_structured"));
        assert_eq!(listed[0].task_id.as_deref(), Some("task_structured"));
        assert_eq!(listed[0].status, Some(ReportStatus::Partial));
        assert_eq!(listed[0].changed_files.as_deref().unwrap().len(), 2);

        // Through serde JSON, every field round-trips byte-for-byte.
        let json = serde_json::to_string(&stored).unwrap();
        let back: AgentReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.plan_id.as_deref(), Some("pl_structured"));
        assert_eq!(back.task_id.as_deref(), Some("task_structured"));
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
    fn report_exposes_actual_requested_and_runtime_mismatch() {
        let mismatch = crate::multiplexer::types::ModelIdentityMismatch {
            planned_model: Some("codex/gpt-5.6-luna".into()),
            requested_model: "gpt-5.6-luna".into(),
            provider_resolved_model: Some("gpt-5.6-luna".into()),
            actual_model: "gpt-5.6-terra".into(),
            reason: "runtime_session_model_mismatch".into(),
        };
        let mut store = ReportStore::new(100);
        let report = store.add_detailed_with_model_identity(
            "orch",
            "w1",
            "Worker",
            "done",
            ReportDetails::default(),
            Some(ReportModelIdentity {
                actual_model: Some("gpt-5.6-terra".into()),
                planned_model: Some("codex/gpt-5.6-luna".into()),
                requested_model: Some("gpt-5.6-luna".into()),
                provider_resolved_model: Some("gpt-5.6-luna".into()),
                fallback: Some(crate::multiplexer::types::ModelFallbackNotice {
                    planned_model: Some("codex/gpt-5.6-luna".into()),
                    requested_model: "gpt-5.6-luna".into(),
                    provider_resolved_model: "gpt-5.6-luna".into(),
                    actual_model: Some("gpt-5.6-terra".into()),
                    reason: "runtime_session_model_mismatch".into(),
                }),
                mismatch: Some(mismatch.clone()),
                reasoning_effort: crate::multiplexer::types::ReasoningEffortIdentity {
                    planned: Some("high".into()),
                    requested: Some("high".into()),
                    resolved: Some("high".into()),
                    actual: None,
                    actual_source: Some("provider-default-unverified".into()),
                },
            }),
        );

        assert_eq!(report.model.as_deref(), Some("gpt-5.6-terra"));
        assert_eq!(report.actual_model.as_deref(), Some("gpt-5.6-terra"));
        assert_eq!(report.planned_model.as_deref(), Some("codex/gpt-5.6-luna"));
        assert_eq!(report.requested_model.as_deref(), Some("gpt-5.6-luna"));
        assert_eq!(
            report.provider_resolved_model.as_deref(),
            Some("gpt-5.6-luna")
        );
        assert!(report.model_fallback.is_some());
        assert_eq!(report.model_identity_mismatch, Some(mismatch));
        assert_eq!(report.reasoning_effort.requested.as_deref(), Some("high"));
        assert_eq!(report.reasoning_effort.actual, None);
        assert_eq!(
            report.reasoning_effort.actual_source.as_deref(),
            Some("provider-default-unverified")
        );
    }

    #[test]
    fn message_only_report_omits_structured_keys_in_json() {
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
            "plan_id",
            "task_id",
            "branch",
            "sha",
            "changed_files",
            "test_cmd",
            "test_result",
            "skipped",
            "model",
            "planned_model",
            "requested_model",
            "provider_resolved_model",
            "actual_model",
            "model_fallback",
            "model_identity_mismatch",
            "reasoning_effort",
        ] {
            assert!(
                json.get(key).is_none(),
                "message-only report must not serialize key {key}"
            );
        }
    }

    #[test]
    fn message_only_report_injects_just_the_message() {
        // proving no regression for P1's synthetic message-only reports.
        let mut store = ReportStore::new(100);
        let report = store.add("orch", "w1", "Worker", "just the body");
        assert_eq!(report.inject_body(), "just the body");
    }

    #[test]
    fn structured_inject_block_renders_in_stable_order() {
        let details = ReportDetails {
            plan_id: Some("pl_1".to_string()),
            task_id: Some("task_1".to_string()),
            owner_epoch: Some(2),
            caller_tab_id: Some("w1".into()),
            caller_session_id: Some("session".into()),
            session_binding_nonce: Some("nonce".into()),
            attempt_id: None,
            status: Some(ReportStatus::Done),
            branch: Some("feature-x".to_string()),
            base: None,
            sha: Some("abc123".to_string()),
            changed_files: Some(vec!["src/a.rs".to_string(), "src/b.rs".to_string()]),
            test_cmd: Some("cargo test".to_string()),
            test_result: Some("42 passed".to_string()),
            skipped: Some("manual UI step".to_string()),
            attachments: vec![],
        };
        let mut store = ReportStore::new(100);
        let report = store.add_detailed("orch", "w1", "Worker", "the freeform body", details, None);
        let expected = "task: pl_1/task_1\n\
             status: Done\n\
             branch: feature-x\n\
             sha: abc123\n\
             tests: cargo test -> 42 passed\n\
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
        assert_eq!(
            serde_json::to_string(&ReportStatus::InReview).unwrap(),
            "\"in_review\""
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
    fn spawn_audit_row_gives_roleless_pane_a_durable_trail() {
        use crate::agent::ledger::OrchestrationLedger;
        let db = std::env::temp_dir().join(format!(
            "reports-audit-{}/orchestration.db",
            ulid::Ulid::new()
        ));
        let ledger = std::sync::Arc::new(OrchestrationLedger::open_at(&db).unwrap());
        let store = ReportStore::with_ledger(100, ledger);
        store.record_spawn_audit("parent_tab", "roleless_tab", "install watchdog", 77);
        let rows = store.ledger_rows("parent_tab");
        assert_eq!(
            rows.len(),
            1,
            "the roleless spawn is now tracked under its parent"
        );
        assert_eq!(rows[0].worker_tab_id, "roleless_tab");
        assert_eq!(rows[0].title, "install watchdog");
        assert_eq!(rows[0].updated_at, 77);
        std::fs::remove_dir_all(db.parent().unwrap()).ok();
    }

    #[test]
    fn completed_rows_surfaces_finished_lanes_only() {
        use crate::agent::ledger::OrchestrationLedger;
        let db = std::env::temp_dir().join(format!(
            "reports-done-{}/orchestration.db",
            ulid::Ulid::new()
        ));
        let ledger = std::sync::Arc::new(OrchestrationLedger::open_at(&db).unwrap());
        let mut store = ReportStore::with_ledger(100, ledger);
        let mut done = ReportDetails::default();
        done.status = Some(ReportStatus::Done);
        store.add_detailed("orch", "w_done", "Worker", "landed", done, None);
        let mut blocked = ReportDetails::default();
        blocked.status = Some(ReportStatus::Blocked);
        store.add_detailed("orch", "w_blocked", "Worker", "stuck", blocked, None);

        let completed = store.completed_rows(50);
        assert_eq!(
            completed.len(),
            1,
            "only finished lanes appear on the Done surface"
        );
        assert_eq!(completed[0].worker_tab_id, "w_done");
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
