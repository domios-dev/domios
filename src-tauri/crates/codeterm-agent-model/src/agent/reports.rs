/// Inter-agent reporting system for orchestration hierarchy.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::ledger::{LedgerRow, OrchestrationLedger};

mod verdict_footer;

pub use codeterm_types::agent::ReportStatus;

pub const BLOCK_PROPOSAL_NOTICE: &str =
    "BLOCK PROPOSAL: task state unchanged pending Plan owner or Planner action";

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportSenderPosition {
    pub source_session_id: String,
    pub message_id: String,
}

#[derive(Debug, Clone, Default)]
pub struct AgentReportHostMetadata {
    pub sender_position: Option<ReportSenderPosition>,
    pub kind: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_position: Option<ReportSenderPosition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
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
    /// Render structured evidence, the freeform message and a task-bound verdict footer.
    /// Reports without structured fields retain their original message.
    pub fn inject_body(&self) -> String {
        let mut lines: Vec<String> = Vec::new();
        match (self.plan_id.as_deref(), self.task_id.as_deref()) {
            (Some(plan), Some(task)) => lines.push(format!("task: {plan}/{task}")),
            (Some(plan), None) => lines.push(format!("plan: {plan}")),
            (None, Some(task)) => lines.push(format!("task: {task}")),
            (None, None) => {}
        }
        let transition_rejected = self
            .report_outcome
            .is_some_and(|outcome| outcome.rejects_task_transition());
        if let Some(status) = self.status {
            if transition_rejected {
                lines.push(format!(
                    "status: Rejected ({} was requested; the task did not move)",
                    status.label()
                ));
            } else if status == ReportStatus::Blocked {
                lines.push(BLOCK_PROPOSAL_NOTICE.into());
            } else {
                lines.push(format!("status: {}", status.label()));
            }
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
        let body = if lines.is_empty() {
            self.message.clone()
        } else {
            format!("{}\n{}", lines.join("\n"), self.message)
        };
        verdict_footer::append(
            &body,
            self.plan_id.as_deref(),
            self.task_id.as_deref(),
            self.status.filter(|_| !transition_rejected),
            self.timestamp,
        )
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
    outbound_by_sender: HashMap<String, Vec<OutboundAgentReport>>,
    delivered: HashSet<String>,
    in_flight: HashSet<String>,
    max_reports_per_orchestrator: usize,
    ledger: Option<Arc<OrchestrationLedger>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundAgentReport {
    pub target_tab_id: String,
    pub report: AgentReport,
}

impl ReportStore {
    pub fn new(max_per_orchestrator: usize) -> Self {
        Self {
            reports: HashMap::new(),
            outbound_by_sender: HashMap::new(),
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
            outbound_by_sender: HashMap::new(),
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

    pub fn add_detailed_with_host_metadata(
        &mut self,
        orchestrator_tab_id: &str,
        from_tab_id: &str,
        from_title: &str,
        message: &str,
        details: ReportDetails,
        model: Option<&str>,
        host_metadata: AgentReportHostMetadata,
    ) -> AgentReport {
        let model_identity = model.map(|actual_model| ReportModelIdentity {
            actual_model: Some(actual_model.to_string()),
            planned_model: None,
            requested_model: None,
            provider_resolved_model: None,
            fallback: None,
            mismatch: None,
            reasoning_effort: Default::default(),
        });
        self.add_detailed_with_model_identity_and_outcome_and_host_metadata(
            orchestrator_tab_id,
            from_tab_id,
            from_title,
            message,
            details,
            model_identity,
            None,
            host_metadata,
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
        self.add_detailed_with_model_identity_and_outcome_and_host_metadata(
            orchestrator_tab_id,
            from_tab_id,
            from_title,
            message,
            details,
            model_identity,
            report_outcome,
            AgentReportHostMetadata::default(),
        )
    }

    pub fn add_detailed_with_model_identity_and_outcome_and_host_metadata(
        &mut self,
        orchestrator_tab_id: &str,
        from_tab_id: &str,
        from_title: &str,
        message: &str,
        details: ReportDetails,
        model_identity: Option<ReportModelIdentity>,
        report_outcome: Option<codeterm_types::agent::ReportOutcomeClass>,
        host_metadata: AgentReportHostMetadata,
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
            sender_position: host_metadata.sender_position,
            kind: host_metadata.kind,
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
        {
            let entries = self
                .outbound_by_sender
                .entry(from_tab_id.to_string())
                .or_default();
            entries.push(OutboundAgentReport {
                target_tab_id: orchestrator_tab_id.to_string(),
                report: report.clone(),
            });
            let excess = entries.len().saturating_sub(self.max_reports_per_orchestrator);
            if excess > 0 {
                entries.drain(..excess);
            }
        }
        if self
            .outbound_by_sender
            .get(from_tab_id)
            .is_some_and(Vec::is_empty)
        {
            self.outbound_by_sender.remove(from_tab_id);
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

    /// Non-consuming sender-owned report snapshot, bounded independently from recipient receipts.
    pub fn outbound_for_sender(&self, sender_tab_id: &str) -> Vec<OutboundAgentReport> {
        self.outbound_by_sender
            .get(sender_tab_id)
            .cloned()
            .unwrap_or_default()
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

    pub fn delivery_trace(&self, report_id: &str) -> Option<(&str, Option<&str>)> {
        if self.delivered.contains(report_id) {
            return None;
        }
        self.reports.iter().find_map(|(target, reports)| {
            reports
                .iter()
                .find(|report| report.id == report_id)
                .map(|report| (target.as_str(), report.attempt_id.as_deref()))
        })
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
        self.outbound_by_sender.remove(orchestrator_tab_id);
        for outbound in self.outbound_by_sender.values_mut() {
            outbound.retain(|entry| entry.target_tab_id != orchestrator_tab_id);
        }
        self.outbound_by_sender
            .retain(|_, outbound| !outbound.is_empty());
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
mod tests;
