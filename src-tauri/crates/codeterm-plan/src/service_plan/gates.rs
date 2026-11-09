use super::store_db::hot_write::{GateAsk, GateChange};
use super::{PlanError, PlanStore};
#[cfg(test)]
use crate::plan::gates::GateStatus;
use crate::plan::gates::{DecisionGate, STALE_GATE_MS};
use crate::plan::{PlannerRequest, TaskStatus};

impl PlanStore {
    pub fn ask_gate(
        &self,
        plan_id: &str,
        question: &str,
        context: &str,
        blocking_task_ids: &[String],
        routed_to_tab_id: Option<String>,
        asked_by_tab_id: Option<&str>,
        now_ms: u64,
    ) -> Result<DecisionGate, PlanError> {
        self.commit_gate_ask(
            plan_id,
            question,
            context,
            blocking_task_ids,
            routed_to_tab_id,
            asked_by_tab_id,
            None,
            false,
            now_ms,
        )
        .map(|(gate, _)| gate)
    }

    pub fn ask_gate_with_planner_request(
        &self,
        plan_id: &str,
        question: &str,
        context: &str,
        blocking_task_ids: &[String],
        routed_to_tab_id: Option<String>,
        asked_by_tab_id: Option<&str>,
        caller_session_id: Option<&str>,
        now_ms: u64,
    ) -> Result<(DecisionGate, PlannerRequest), PlanError> {
        let (gate, request) = self.commit_gate_ask(
            plan_id,
            question,
            context,
            blocking_task_ids,
            routed_to_tab_id,
            asked_by_tab_id,
            caller_session_id,
            true,
            now_ms,
        )?;
        Ok((gate, request.ok_or(PlanError::GateNotFound)?))
    }

    fn commit_gate_ask(
        &self,
        plan_id: &str,
        question: &str,
        context: &str,
        blocking_task_ids: &[String],
        routed_to_tab_id: Option<String>,
        asked_by_tab_id: Option<&str>,
        caller_session_id: Option<&str>,
        planner_request: bool,
        now_ms: u64,
    ) -> Result<(DecisionGate, Option<PlannerRequest>), PlanError> {
        let question = question.trim();
        if question.is_empty() {
            return Err(PlanError::GateRequiresQuestion);
        }
        let turn = self.mutation_writer.enter();
        let (gate, request, receipt) = self.database.ask_gate_rows(
            plan_id,
            GateAsk {
                question: question.into(),
                context: context.into(),
                blocking_task_ids: blocking_task_ids.to_vec(),
                routed_to_tab_id,
                asked_by_tab_id: asked_by_tab_id.map(str::to_string),
                caller_session_id: caller_session_id.map(str::to_string),
                planner_request,
            },
            now_ms,
        )?;
        self.publish_hot_receipt(&turn, receipt);
        Ok((gate, request))
    }

    pub fn answer_gate(
        &self,
        plan_id: &str,
        gate_id: &str,
        answer_text: &str,
        answered_by_tab_id: Option<&str>,
        now_ms: u64,
    ) -> Result<DecisionGate, PlanError> {
        self.commit_gate_change(
            plan_id,
            gate_id,
            GateChange::Answer {
                text: answer_text.into(),
                by_tab_id: answered_by_tab_id.map(str::to_string),
            },
            now_ms,
        )
    }

    pub fn list_gates(
        &self,
        plan_id: &str,
        open_only: bool,
    ) -> Result<Vec<DecisionGate>, PlanError> {
        self.database.gate_rows(plan_id, open_only, None)
    }

    pub fn get_gate(&self, plan_id: &str, gate_id: &str) -> Result<DecisionGate, PlanError> {
        self.database
            .gate_rows(plan_id, false, Some(gate_id))?
            .into_iter()
            .next()
            .ok_or(PlanError::GateNotFound)
    }

    pub fn escalate_gate(
        &self,
        plan_id: &str,
        gate_id: &str,
        note: Option<&str>,
        now_ms: u64,
    ) -> Result<DecisionGate, PlanError> {
        self.commit_gate_change(
            plan_id,
            gate_id,
            GateChange::Escalate {
                note: note.map(str::to_string),
            },
            now_ms,
        )
    }

    fn commit_gate_change(
        &self,
        plan_id: &str,
        gate_id: &str,
        change: GateChange,
        now_ms: u64,
    ) -> Result<DecisionGate, PlanError> {
        let turn = self.mutation_writer.enter();
        let (gate, receipt) = self
            .database
            .change_gate(plan_id, gate_id, change, now_ms)?;
        self.publish_hot_receipt(&turn, receipt);
        Ok(gate)
    }
}

/// Doctor findings about gate health — used in the plan doctor.
#[derive(Debug, Clone)]
pub struct GateFinding {
    pub severity: &'static str,
    pub kind: &'static str,
    pub gate_id: Option<String>,
    pub task_id: Option<String>,
    pub detail: String,
}

pub fn gate_doctor_findings(plan: &crate::plan::Plan, now_ms: u64) -> Vec<GateFinding> {
    let mut findings = Vec::new();

    // Blocked/paused task without a gate reference or pause_reason → error.
    for task in &plan.tasks {
        if !matches!(task.status, TaskStatus::Blocked | TaskStatus::Paused) {
            continue;
        }
        if task.pause_reason.as_deref().is_some_and(|r| !r.is_empty()) {
            continue;
        }
        findings.push(GateFinding {
            severity: "error",
            kind: "paused_task_missing_reason",
            gate_id: None,
            task_id: Some(task.id.clone()),
            detail: format!(
                "task {} ({}) is {} but has no pause_reason or gate reference",
                task.id,
                task.name,
                serde_json::to_value(&task.status)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_default()
            ),
        });
    }

    // Stale open gate → warning.
    for gate in &plan.gates {
        if gate.is_stale(now_ms) {
            let age_h = now_ms.saturating_sub(gate.asked_at) / 3_600_000;
            findings.push(GateFinding {
                severity: "warning",
                kind: "stale_open_gate",
                gate_id: Some(gate.id.clone()),
                task_id: None,
                detail: format!(
                    "gate {} is open for {age_h}h — question: {}",
                    gate.id,
                    gate.question.chars().take(80).collect::<String>()
                ),
            });
        }
    }

    findings
}

// Threshold for what "stale" means in gate doctor warnings.
#[allow(dead_code)]
const _STALE_GATE_MS_DOC: u64 = STALE_GATE_MS;

#[cfg(test)]
mod tests;
