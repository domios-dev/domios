use crate::plan::gates::{DecisionGate, GateAnswer, GateStatus, STALE_GATE_MS};
use crate::plan::ids::mint_id;
use crate::plan::{AgentRef, Milestone, TaskStatus};

use super::mutations::plan_contains_id;
use super::{PlanError, PlanStore};

fn gate_id_taken(plan: &crate::plan::Plan, candidate: &str) -> bool {
    plan_contains_id(plan, candidate) || plan.gates.iter().any(|g| g.id == candidate)
}

/// Create a decision gate, park all `blocking_task_ids`, and return the gate.
/// `routed_to_pane_id` is resolved by the caller via `resolve_planner_ref` before this call.
/// Delivery to that pane is handled by `ServiceContext` after this returns.
impl PlanStore {
    pub fn ask_gate(
        &self,
        plan_id: &str,
        question: &str,
        context: &str,
        blocking_task_ids: &[String],
        routed_to_pane_id: Option<String>,
        asked_by_pane_id: Option<&str>,
        now_ms: u64,
    ) -> Result<DecisionGate, PlanError> {
        let question = question.trim();
        if question.is_empty() {
            return Err(PlanError::GateRequiresQuestion);
        }
        let mut result: Option<DecisionGate> = None;
        self.with_plan(plan_id, now_ms, |plan| {
            let gid = mint_id("gate", |c| gate_id_taken(plan, c));

            // Park blocking tasks.
            let park_reason = format!("gate:{gid}: {}", question.chars().take(80).collect::<String>());
            for tid in blocking_task_ids {
                if let Some(task) = plan
                    .tasks
                    .iter_mut()
                    .find(|t| &t.id == tid || t.handle.as_deref() == Some(tid))
                {
                    if !task.status.is_terminal() {
                        let mid = mint_id("ms", |c| plan_contains_id(plan, c));
                        task.status = TaskStatus::Parked;
                        task.park_reason = Some(park_reason.clone());
                        task.status_changed_at = now_ms;
                        task.updated_at = now_ms;
                        task.milestones.push(Milestone {
                            id: mid,
                            from: AgentRef::from_pane(asked_by_pane_id.unwrap_or("orchestrator")),
                            kind: "decision".into(),
                            text: format!("parked: {park_reason}"),
                            ts: now_ms,
                            links: vec![],
                            loop_id: None,
                            iteration: None,
                            canvas: Default::default(),
                            attachments: vec![],
                        });
                    }
                }
            }

            let gate = DecisionGate {
                id: gid,
                question: question.to_string(),
                context: context.trim().to_string(),
                blocking_task_ids: blocking_task_ids.to_vec(),
                routed_to_pane_id: routed_to_pane_id.clone(),
                asked_by_pane_id: asked_by_pane_id.map(str::to_string),
                asked_at: now_ms,
                status: GateStatus::Open,
                answer: None,
                escalated_at: None,
                escalation_note: None,
            };
            plan.gates.push(gate.clone());
            result = Some(gate);
            Ok(())
        })?;
        result.ok_or_else(|| PlanError::Io("ask_gate produced no result".into()))
    }

    /// Record an answer, unpark blocking tasks, return the updated gate.
    /// Nudging the asker pane is handled by `ServiceContext`.
    pub fn answer_gate(
        &self,
        plan_id: &str,
        gate_id: &str,
        answer_text: &str,
        answered_by_pane_id: Option<&str>,
        now_ms: u64,
    ) -> Result<DecisionGate, PlanError> {
        let answer_text = answer_text.trim();
        let mut out: Option<DecisionGate> = None;
        self.with_plan(plan_id, now_ms, |plan| {
            let gate = plan
                .gates
                .iter_mut()
                .find(|g| g.id == gate_id)
                .ok_or(PlanError::GateNotFound)?;

            if !gate.is_open() {
                return Err(PlanError::GateAlreadyAnswered);
            }

            gate.answer = Some(GateAnswer {
                text: answer_text.to_string(),
                by_pane_id: answered_by_pane_id.map(str::to_string),
                at: now_ms,
            });
            gate.status = GateStatus::Answered;

            let blocking = gate.blocking_task_ids.clone();
            let gid = gate.id.clone();
            let gate_snapshot = gate.clone();
            out = Some(gate_snapshot);

            // Unpark blocked tasks.
            for tid in &blocking {
                if let Some(task) = plan
                    .tasks
                    .iter_mut()
                    .find(|t| &t.id == tid || t.handle.as_deref() == Some(tid))
                {
                    if task.status == TaskStatus::Parked {
                        let mid = mint_id("ms", |c| plan_contains_id(plan, c));
                        task.status = TaskStatus::Pending;
                        task.status_changed_at = now_ms;
                        task.park_reason = None;
                        task.updated_at = now_ms;
                        task.milestones.push(Milestone {
                            id: mid,
                            from: AgentRef::from_pane(
                                answered_by_pane_id.unwrap_or("planner"),
                            ),
                            kind: "decision".into(),
                            text: format!("unparked: gate {gid} answered"),
                            ts: now_ms,
                            links: vec![gid.clone()],
                            loop_id: None,
                            iteration: None,
                            canvas: Default::default(),
                            attachments: vec![],
                        });
                    }
                }
            }
            Ok(())
        })?;
        out.ok_or_else(|| PlanError::Io("answer_gate produced no result".into()))
    }

    pub fn list_gates(
        &self,
        plan_id: &str,
        open_only: bool,
    ) -> Result<Vec<DecisionGate>, PlanError> {
        let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
        let gates = plan
            .gates
            .iter()
            .filter(|g| !open_only || g.is_open())
            .cloned()
            .collect();
        Ok(gates)
    }

    pub fn get_gate(&self, plan_id: &str, gate_id: &str) -> Result<DecisionGate, PlanError> {
        let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
        plan.gates
            .iter()
            .find(|g| g.id == gate_id)
            .cloned()
            .ok_or(PlanError::GateNotFound)
    }

    /// Re-ping and optionally escalate a stale open gate. Returns updated gate.
    pub fn escalate_gate(
        &self,
        plan_id: &str,
        gate_id: &str,
        note: Option<&str>,
        now_ms: u64,
    ) -> Result<DecisionGate, PlanError> {
        let mut out: Option<DecisionGate> = None;
        self.with_plan(plan_id, now_ms, |plan| {
            let gate = plan
                .gates
                .iter_mut()
                .find(|g| g.id == gate_id)
                .ok_or(PlanError::GateNotFound)?;
            if !gate.is_open() {
                return Err(PlanError::GateAlreadyAnswered);
            }
            gate.escalated_at = Some(now_ms);
            gate.status = GateStatus::Escalated;
            if let Some(n) = note {
                let n = n.trim();
                if !n.is_empty() {
                    gate.escalation_note = Some(n.to_string());
                }
            }
            out = Some(gate.clone());
            Ok(())
        })?;
        out.ok_or_else(|| PlanError::Io("escalate_gate produced no result".into()))
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

    // Blocked/parked task without gate ref or park_reason → error.
    for task in &plan.tasks {
        if !matches!(task.status, TaskStatus::Blocked | TaskStatus::Parked) {
            continue;
        }
        if task.park_reason.as_deref().is_some_and(|r| !r.is_empty()) {
            continue;
        }
        findings.push(GateFinding {
            severity: "error",
            kind: "parked_task_missing_reason",
            gate_id: None,
            task_id: Some(task.id.clone()),
            detail: format!(
                "task {} ({}) is {} but has no park_reason or gate reference",
                task.id, task.name, serde_json::to_value(&task.status).ok()
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
                    gate.id, gate.question.chars().take(80).collect::<String>()
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
mod tests {
    use super::*;
    use crate::plan::{OrchestratorRef, Plan};
    use crate::service::plan::{NewTask, PlanStore};
    use tempfile::TempDir;

    fn store_with_tasks(names: &[&str]) -> (TempDir, PlanStore, String, Vec<String>) {
        let dir = TempDir::new().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "Gate test".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_pane("orch_pane"),
        );
        store.save(&mut plan, 1).unwrap();
        let task_ids = names
            .iter()
            .map(|n| {
                store
                    .add_task(
                        &plan.id,
                        NewTask {
                            name: (*n).into(),
                            kind: "implement".into(),
                            ..Default::default()
                        },
                        2,
                    )
                    .unwrap()
            })
            .collect();
        (dir, store, plan.id, task_ids)
    }

    #[test]
    fn ask_gate_parks_blocking_tasks_only() {
        let (_dir, store, pid, tids) =
            store_with_tasks(&["task-one", "task-two", "task-three"]);
        let (t1, t2, t3) = (tids[0].clone(), tids[1].clone(), tids[2].clone());

        let gate = store
            .ask_gate(&pid, "Which direction?", "", &[t1.clone()], None, Some("orch_pane"), 1000)
            .unwrap();
        assert!(gate.is_open());
        assert_eq!(gate.blocking_task_ids, vec![t1.clone()]);

        let plan = store.get(&pid).unwrap();
        let task1 = plan.tasks.iter().find(|t| t.id == t1).unwrap();
        assert_eq!(task1.status, TaskStatus::Parked);
        assert!(task1.park_reason.as_deref().unwrap().starts_with("gate:gate_"));

        // t2 and t3 still pending — frontier not frozen
        let task2 = plan.tasks.iter().find(|t| t.id == t2).unwrap();
        let task3 = plan.tasks.iter().find(|t| t.id == t3).unwrap();
        assert_eq!(task2.status, TaskStatus::Pending);
        assert_eq!(task3.status, TaskStatus::Pending);

        // Plan still has open work — never fully frozen by one gate
        assert!(plan.tasks.iter().any(|t| t.status.counts_as_open_work()));
    }

    #[test]
    fn answer_gate_unparks_tasks_and_is_immutable() {
        let (_dir, store, pid, tids) = store_with_tasks(&["blocked-task"]);
        let t1 = tids[0].clone();

        let gate = store
            .ask_gate(&pid, "Decision?", "", &[t1.clone()], None, None, 1000)
            .unwrap();
        let gid = gate.id.clone();

        let answered = store
            .answer_gate(&pid, &gid, "Go left.", Some("planner_pane"), 2000)
            .unwrap();
        assert_eq!(answered.status, GateStatus::Answered);
        assert_eq!(answered.answer.as_ref().unwrap().text, "Go left.");

        let plan = store.get(&pid).unwrap();
        let task1 = plan.tasks.iter().find(|t| t.id == t1).unwrap();
        assert_eq!(task1.status, TaskStatus::Pending, "task must be unparked after answer");

        // Second answer must fail (immutable audit record)
        let err = store
            .answer_gate(&pid, &gid, "Nope.", None, 3000)
            .unwrap_err();
        assert!(matches!(err, PlanError::GateAlreadyAnswered));
    }

    #[test]
    fn empty_question_is_rejected() {
        let (_dir, store, pid, _) = store_with_tasks(&[]);
        let err = store
            .ask_gate(&pid, "  ", "", &[], None, None, 1000)
            .unwrap_err();
        assert!(matches!(err, PlanError::GateRequiresQuestion));
    }

    #[test]
    fn plan_never_fully_frozen_by_one_gate() {
        let (_dir, store, pid, tids) = store_with_tasks(&["blocked", "free"]);
        store
            .ask_gate(&pid, "Question?", "", &[tids[0].clone()], None, None, 1000)
            .unwrap();
        let plan = store.get(&pid).unwrap();
        // tids[1] is still open work even though tids[0] is parked
        assert!(
            plan.tasks.iter().any(|t| t.status.counts_as_open_work()),
            "plan must retain open work so frontier keeps dispatching"
        );
    }

    #[test]
    fn gate_doctor_flags_parked_task_without_reason() {
        let mut plan = Plan::new("T".into(), "/p".into(), OrchestratorRef::from_pane("p"));
        plan.tasks.push(crate::plan::Task {
            id: "task_x".into(),
            name: "x".into(),
            kind: "implement".into(),
            status: TaskStatus::Parked,
            park_reason: None,
            model: None,
            model_rationale: None,
            acceptance_criteria: vec![],
            test_plan: None,
            correctness: None,
            review_stage: Default::default(),
            qa_state: None,
            status_label: None,
            priority: Default::default(),
            created_at: 0,
            updated_at: 0,
            status_changed_at: 0,
            handle: None,
            tags: vec![],
            section_ids: vec![],
            body: String::new(),
            role_guidance: None,
            executor_role: None,
            competition: None,
            intent: None,
            parent_task_id: None,
            assigned_pane_id: None,
            lane: Default::default(),
            taken_by_role: None,
            peer: None,
            group: None,
            canvas: Default::default(),
            milestones: vec![],
            execution_attempts: vec![],
            attachments: vec![],
        });
        let findings = gate_doctor_findings(&plan, 1_000);
        assert!(findings
            .iter()
            .any(|f| f.kind == "parked_task_missing_reason" && f.severity == "error"));
    }

    #[test]
    fn gate_doctor_warns_on_stale_open_gate() {
        let mut plan = Plan::new("T".into(), "/p".into(), Default::default());
        plan.gates.push(DecisionGate {
            id: "gate_stale".into(),
            question: "Stale question?".into(),
            context: String::new(),
            blocking_task_ids: vec![],
            routed_to_pane_id: None,
            asked_by_pane_id: None,
            asked_at: 0,
            status: GateStatus::Open,
            answer: None,
            escalated_at: None,
            escalation_note: None,
        });
        // now_ms > STALE_GATE_MS so gate is stale
        let findings = gate_doctor_findings(&plan, STALE_GATE_MS + 1);
        assert!(findings.iter().any(|f| f.kind == "stale_open_gate" && f.severity == "warning"));
    }

}
