use serde::{Deserialize, Serialize};

use crate::plan::{Task, TaskCompetitionMetadata, TaskExecutionAttempt};

use super::{PlanError, PlanStore};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskCompetitionUpdate {
    pub enabled: Option<bool>,
    pub attempts: Option<Option<u32>>,
    pub models: Option<Vec<String>>,
    pub judge_hint: Option<Option<String>>,
}

pub const COMPETITIVE_DISPATCH_NUDGE: &str =
    "This task is marked competitive; run grouped competition dispatch before single-worker dispatch.";
pub const COMPETITIVE_SINGLE_DISPATCH_WARNING: &str =
    "competitive task has a single live attempt; dispatch through the plan.task.compete route or record an explicit abandonment";
pub const COMPETITION_WORKER_ISOLATION_PROMPT: &str =
    "You are one isolated competitive attempt. Work only in your assigned lane, do not inspect peer attempts, and report honestly for orchestrator judging.";

pub fn is_competitive(task: &Task) -> bool {
    task.competition
        .as_ref()
        .is_some_and(|competition| competition.enabled)
}

pub fn competition_attempts(task: &Task) -> u32 {
    task.competition
        .as_ref()
        .and_then(|competition| competition.attempts)
        .unwrap_or(2)
        .max(2)
}

pub fn live_competition_attempt_count(task: &Task) -> usize {
    task.execution_attempts
        .iter()
        .filter(|attempt| attempt.competition_group_id.is_some() && attempt.closed_at.is_none())
        .count()
}

pub fn competition_summary(task: &Task) -> Option<String> {
    let competition = task.competition.as_ref().filter(|c| c.enabled)?;
    let attempts = competition.attempts.unwrap_or(2).max(2);
    let mut parts = vec![format!("{attempts} attempts")];
    if !competition.models.is_empty() {
        parts.push(format!("models {}", competition.models.join(",")));
    }
    if let Some(hint) = competition
        .judge_hint
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        parts.push(format!("judge: {hint}"));
    }
    Some(parts.join("; "))
}

pub fn apply_competition_update(
    current: &mut Option<TaskCompetitionMetadata>,
    update: TaskCompetitionUpdate,
) {
    let mut next = current.clone().unwrap_or_default();
    if let Some(enabled) = update.enabled {
        next.enabled = enabled;
    }
    if let Some(attempts) = update.attempts {
        next.attempts = attempts.map(|n| n.max(2));
    }
    if let Some(models) = update.models {
        next.models = models
            .into_iter()
            .map(|model| model.trim().to_string())
            .filter(|model| !model.is_empty())
            .collect();
    }
    if let Some(hint) = update.judge_hint {
        next.judge_hint = hint.and_then(|s| {
            let trimmed = s.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        });
    }
    *current = next.enabled.then_some(next);
}

pub fn set_attempt_judgement(
    attempt: &mut TaskExecutionAttempt,
    verdict: &str,
    reason: Option<&str>,
    winner: bool,
    now_ms: u64,
) {
    attempt.judge_verdict = Some(verdict.to_string());
    attempt.judge_reason = reason.map(str::to_string);
    attempt.winner = winner;
    attempt.judged_at = Some(now_ms);
    if !winner && attempt.closed_at.is_none() {
        attempt.closed_at = Some(now_ms);
    }
}

impl PlanStore {
    pub fn update_task_competition(
        &self,
        plan_id: &str,
        task_id: &str,
        update: TaskCompetitionUpdate,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            let task = crate::plan::task_key::find_task_mut(&mut p.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            apply_competition_update(&mut task.competition, update);
            task.updated_at = now_ms;
            Ok(())
        })
    }
}
