use serde::Serialize;

use super::mutations::clear_task_assignment;
use super::{PlanError, PlanStore};
use crate::plan::{
    AgentRef, Milestone, QaState, ReviewEvidence, ReviewStage, ReviewVerdict, Task, TaskStatus,
};

pub const QA_MILESTONE_KIND: &str = "qa";

#[derive(serde::Deserialize)]
pub(super) struct QaQueueTaskRead {
    pub id: String,
    pub name: String,
    pub qa_state: Option<QaState>,
    pub status_changed_at: u64,
    pub assigned_tab_id: Option<String>,
    pub qa_plan: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct QaQueueEntry {
    pub task_id: String,
    pub name: String,
    pub qa_state: QaState,
    pub position: Option<usize>,
    pub started_at: Option<u64>,
    pub age_ms: Option<u64>,
    pub assigned_tab_id: Option<String>,
    pub test_plan: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct QaQueue {
    pub plan_id: String,
    pub queued: Vec<QaQueueEntry>,
    pub running: Vec<QaQueueEntry>,
    pub max_running: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct QaTransitionResult {
    pub plan_id: String,
    pub task_id: String,
    pub milestone_id: String,
    pub task_name: String,
    pub test_plan: Option<String>,
    pub nudged_tab_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy_warning: Option<super::review_gate::StrategyWarning>,
}

fn max_running_qa_per_plan() -> usize {
    std::env::var("CODETERM_PLAN_QA_MAX_RUNNING")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(1)
}

pub fn normalize_task_review_fields(task: &mut Task) {
    if task.status != TaskStatus::InReview {
        task.review_stage = ReviewStage::Review;
        task.qa_state = None;
    } else if task.review_stage != ReviewStage::Qa {
        task.qa_state = None;
    }
}

fn require_note(note: &str) -> Result<&str, PlanError> {
    let note = note.trim();
    if note.is_empty() {
        Err(PlanError::QaNoteRequired)
    } else {
        Ok(note)
    }
}

fn qa_milestone(id: String, from: AgentRef, note: &str, now_ms: u64) -> Milestone {
    Milestone {
        id,
        from,
        kind: QA_MILESTONE_KIND.into(),
        text: note.into(),
        ts: now_ms,
        links: vec![],
        attachments: vec![],
        loop_id: None,
        iteration: None,
        canvas: Default::default(),
    }
}

fn effective_qa_plan(task: &Task) -> Option<String> {
    task.qa_plan.clone().or_else(|| task.test_plan.clone())
}

fn qa_entry(task: &QaQueueTaskRead, position: Option<usize>, now_ms: u64) -> QaQueueEntry {
    let started_at = (task.qa_state == Some(QaState::Running)).then_some(task.status_changed_at);
    QaQueueEntry {
        task_id: task.id.clone(),
        name: task.name.clone(),
        qa_state: task.qa_state.clone().unwrap_or(QaState::Queued),
        position,
        started_at,
        age_ms: started_at.map(|ts| now_ms.saturating_sub(ts)),
        assigned_tab_id: task.assigned_tab_id.clone(),
        test_plan: task.qa_plan.clone(),
    }
}

fn require_running_qa(task: &Task) -> Result<(), PlanError> {
    if task.status == TaskStatus::InReview
        && task.review_stage == ReviewStage::Qa
        && task.qa_state == Some(QaState::Running)
    {
        Ok(())
    } else {
        Err(PlanError::QaNotRunning)
    }
}

fn qa_attempt_error(operation: &str, message: impl Into<String>) -> PlanError {
    PlanError::ReadbackMismatch {
        operation: operation.into(),
        message: message.into(),
    }
}

fn claimable_attempt_index(task: &Task) -> Result<usize, PlanError> {
    let assigned_executor = task
        .assigned_tab_id
        .as_deref()
        .map(str::trim)
        .filter(|tab_id| !tab_id.is_empty());
    let candidates = task
        .execution_attempts
        .iter()
        .enumerate()
        .filter(|(_, attempt)| attempt.closed_at.is_none())
        .filter(|(_, attempt)| match assigned_executor {
            Some(tab_id) => attempt.executor_tab_id == tab_id,
            None => attempt.competition_group_id.is_some() && attempt.winner,
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    match candidates.as_slice() {
        [index] => Ok(*index),
        [] => Err(qa_attempt_error(
            "qa_claim",
            "QA item has no exact live execution attempt owned by its executor",
        )),
        _ => Err(qa_attempt_error(
            "qa_claim",
            "QA item has ambiguous live execution-attempt ownership",
        )),
    }
}

fn claimed_attempt_index(task: &Task) -> Result<usize, PlanError> {
    let candidates = task
        .execution_attempts
        .iter()
        .enumerate()
        .filter(|(_, attempt)| attempt.reviewed_at == Some(task.status_changed_at))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    match candidates.as_slice() {
        [index] => Ok(*index),
        [] => Err(qa_attempt_error(
            "qa_pass",
            "running QA item has no exact claimed execution attempt",
        )),
        _ => Err(qa_attempt_error(
            "qa_pass",
            "running QA item has ambiguous claimed execution attempts",
        )),
    }
}

fn require_current_claimed_attempt(task: &Task, claimed_index: usize) -> Result<(), PlanError> {
    let claimed = &task.execution_attempts[claimed_index];
    if claimed.closed_at.is_some() {
        return Err(qa_attempt_error(
            "qa_pass",
            format!(
                "claimed execution attempt '{}' was superseded or closed during QA",
                claimed.id
            ),
        ));
    }
    if let Some(other) = task
        .execution_attempts
        .iter()
        .enumerate()
        .find(|(index, attempt)| *index != claimed_index && attempt.closed_at.is_none())
        .map(|(_, attempt)| attempt)
    {
        return Err(qa_attempt_error(
            "qa_pass",
            format!(
                "live execution attempt '{}' supersedes or competes with claimed attempt '{}'",
                other.id, claimed.id
            ),
        ));
    }
    Ok(())
}

impl PlanStore {
    pub fn qa_queue(&self, plan_id: &str, now_ms: u64) -> Result<QaQueue, PlanError> {
        let tasks = self.database.qa_queue_tasks(plan_id)?;
        Ok(qa_queue_from_facts(plan_id, &tasks, now_ms))
    }

    pub fn enqueue_qa(
        &self,
        plan_id: &str,
        task_id: &str,
        note: &str,
        from: AgentRef,
        now_ms: u64,
    ) -> Result<QaTransitionResult, PlanError> {
        let note = require_note(note)?;
        self.with_task_rows(
            plan_id,
            task_id,
            super::store_db::hot_write::TaskHistory::Attempts,
            now_ms,
            |edit| {
                if edit.task.status != TaskStatus::InReview {
                    return Err(PlanError::TransitionDenied {
                        from: edit.task.status.clone(),
                        to: TaskStatus::InReview,
                    });
                }
                let mid = crate::plan::ids::short_id("ms");
                let task = &mut edit.task;
                task.review_stage = ReviewStage::Qa;
                task.qa_state = Some(QaState::Queued);
                task.status_changed_at = now_ms;
                task.updated_at = now_ms;
                task.milestones.push(qa_milestone(
                    mid.clone(),
                    from,
                    &format!("enqueue: {note}"),
                    now_ms,
                ));
                Ok(QaTransitionResult {
                    plan_id: plan_id.into(),
                    task_id: task.id.clone(),
                    milestone_id: mid,
                    task_name: task.name.clone(),
                    test_plan: effective_qa_plan(task),
                    nudged_tab_id: None,
                    strategy_warning: None,
                })
            },
        )
    }

    pub fn claim_next_qa(
        &self,
        plan_id: &str,
        note: &str,
        from: AgentRef,
        qa_tab_id: Option<&str>,
        now_ms: u64,
    ) -> Result<Option<QaTransitionResult>, PlanError> {
        let note = require_note(note)?;
        let turn = self.mutation_writer.enter();
        let (result, receipt) = self.database.claim_next_qa_rows(
            plan_id,
            max_running_qa_per_plan(),
            now_ms,
            |edit| {
                let mid = crate::plan::ids::short_id("ms");
                let attempt_index = claimable_attempt_index(&edit.task)?;
                let task = &mut edit.task;
                task.execution_attempts[attempt_index].reviewed_at = Some(now_ms);
                task.qa_state = Some(QaState::Running);
                task.status_changed_at = now_ms;
                task.assigned_tab_id = qa_tab_id.map(str::to_string);
                task.updated_at = now_ms;
                task.milestones.push(qa_milestone(
                    mid.clone(),
                    from,
                    &format!("claim: {note}"),
                    now_ms,
                ));
                Ok(QaTransitionResult {
                    plan_id: plan_id.into(),
                    task_id: task.id.clone(),
                    milestone_id: mid,
                    task_name: task.name.clone(),
                    test_plan: effective_qa_plan(task),
                    nudged_tab_id: None,
                    strategy_warning: None,
                })
            },
        )?;
        self.publish_hot_receipt(&turn, receipt);
        Ok(result)
    }

    pub fn pass_qa(
        &self,
        plan_id: &str,
        task_id: &str,
        note: &str,
        from: AgentRef,
        now_ms: u64,
    ) -> Result<QaTransitionResult, PlanError> {
        let note = require_note(note)?;
        self.with_task_rows(
            plan_id,
            task_id,
            super::store_db::hot_write::TaskHistory::Attempts,
            now_ms,
            |edit| {
                require_running_qa(&edit.task)?;
                let attempt_index = claimed_attempt_index(&edit.task)?;
                require_current_claimed_attempt(&edit.task, attempt_index)?;
                edit.terminal_check = true;
                let mid = crate::plan::ids::short_id("ms");
                let task = &mut edit.task;
                super::task_attempts::accept_exact_attempt(
                    &mut task.execution_attempts[attempt_index],
                    note,
                    now_ms,
                );
                task.status = TaskStatus::Done;
                task.review_stage = ReviewStage::Review;
                task.qa_state = None;
                task.status_changed_at = now_ms;
                clear_task_assignment(task);
                task.updated_at = now_ms;
                task.review_evidence.push(ReviewEvidence {
                    id: crate::plan::ids::short_id("rev"),
                    from: from.clone(),
                    verdict: ReviewVerdict::Accept,
                    note: note.into(),
                    at: now_ms,
                    changed_executable_surfaces: false,
                    distinct_oracle: false,
                });
                task.milestones.push(qa_milestone(
                    mid.clone(),
                    from,
                    &format!("pass: {note}"),
                    now_ms,
                ));
                edit.review_credit = 1;
                Ok(QaTransitionResult {
                    plan_id: plan_id.into(),
                    task_id: task.id.clone(),
                    milestone_id: mid,
                    task_name: task.name.clone(),
                    test_plan: effective_qa_plan(task),
                    nudged_tab_id: None,
                    strategy_warning: None,
                })
            },
        )
    }

    pub fn fail_qa(
        &self,
        plan_id: &str,
        task_id: &str,
        note: &str,
        changed_executable_surfaces: bool,
        distinct_oracle: bool,
        from: AgentRef,
        now_ms: u64,
    ) -> Result<QaTransitionResult, PlanError> {
        let note = require_note(note)?;
        self.with_task_rows(
            plan_id,
            task_id,
            super::store_db::hot_write::TaskHistory::Review,
            now_ms,
            |edit| {
                require_running_qa(&edit.task)?;
                let mid = crate::plan::ids::short_id("ms");
                let implementer = edit
                    .task
                    .execution_attempts
                    .iter()
                    .rev()
                    .find_map(|attempt| {
                        (attempt.report.as_ref().and_then(|r| r.status.as_deref())
                            == Some("in_review"))
                        .then(|| attempt.executor_tab_id.clone())
                    })
                    .or_else(|| edit.task.lane.tab_id.clone())
                    .or_else(|| edit.task.assigned_tab_id.clone());
                let task = &mut edit.task;
                task.status = TaskStatus::InProgress;
                task.review_stage = ReviewStage::Review;
                task.qa_state = None;
                task.status_label = Some("QA failed".into());
                task.status_changed_at = now_ms;
                task.assigned_tab_id = implementer.clone();
                task.lane.tab_id = implementer.clone();
                task.updated_at = now_ms;
                task.review_round += 1;
                task.review_evidence.push(ReviewEvidence {
                    id: crate::plan::ids::short_id("rev"),
                    from: from.clone(),
                    verdict: ReviewVerdict::Bounce,
                    note: note.into(),
                    at: now_ms,
                    changed_executable_surfaces,
                    distinct_oracle,
                });
                task.milestones.push(qa_milestone(
                    mid.clone(),
                    from,
                    &format!("fail: {note}"),
                    now_ms,
                ));
                edit.review_credit = 1;
                Ok(QaTransitionResult {
                    plan_id: plan_id.into(),
                    task_id: task.id.clone(),
                    milestone_id: mid,
                    task_name: task.name.clone(),
                    test_plan: effective_qa_plan(task),
                    nudged_tab_id: implementer,
                    strategy_warning: super::review_gate::strategy_warning(task),
                })
            },
        )
    }
}

#[cfg(test)]
pub(super) fn qa_queue_for(plan_id: &str, tasks: &[Task], now_ms: u64) -> QaQueue {
    let facts = tasks
        .iter()
        .filter(|task| task.status == TaskStatus::InReview && task.review_stage == ReviewStage::Qa)
        .map(|task| QaQueueTaskRead {
            id: task.id.clone(),
            name: task.name.clone(),
            qa_state: task.qa_state.clone(),
            status_changed_at: task.status_changed_at,
            assigned_tab_id: task.assigned_tab_id.clone(),
            qa_plan: effective_qa_plan(task),
        })
        .collect::<Vec<_>>();
    qa_queue_from_facts(plan_id, &facts, now_ms)
}

pub(super) fn qa_queue_from_facts(
    plan_id: &str,
    tasks: &[QaQueueTaskRead],
    now_ms: u64,
) -> QaQueue {
    let mut queued = Vec::new();
    let mut running = Vec::new();
    for task in tasks {
        match task.qa_state {
            Some(QaState::Running) => running.push(qa_entry(task, None, now_ms)),
            Some(QaState::Queued) | None => {
                queued.push(qa_entry(task, Some(queued.len() + 1), now_ms));
            }
        }
    }
    QaQueue {
        plan_id: plan_id.to_string(),
        queued,
        running,
        max_running: max_running_qa_per_plan(),
    }
}

#[cfg(test)]
mod tests;
