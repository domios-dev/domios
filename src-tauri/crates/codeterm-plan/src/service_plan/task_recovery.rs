use crate::plan::{Task, TaskStatus};

use super::{PlanError, PlanStore};

pub const OWNER_RETIRED_LABEL: &str = "owner_retired";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleOwnerRetirement {
    pub retired_attempt_ids: Vec<String>,
    pub owner_epoch: u64,
    pub previous_owner_tab_id: Option<String>,
    /// False when the task was already retired — the call is a no-op, so a second
    /// authority pass in the same request never bumps the epoch twice.
    pub applied: bool,
}

/// Retire the attempts this tab holds LIVE; closed history is left alone.
/// One-way fence: a retired attempt can never re-enter startup or commit.
pub fn retire_attempts_for_tab(task: &mut Task, tab_id: &str, now_ms: u64) -> Vec<String> {
    retire_attempts(task, now_ms, |attempt| {
        attempt.executor_tab_id == tab_id && attempt.closed_at.is_none()
    })
}

fn retire_attempts<F>(task: &mut Task, now_ms: u64, mut selector: F) -> Vec<String>
where
    F: FnMut(&crate::plan::TaskExecutionAttempt) -> bool,
{
    let mut retired = Vec::new();
    for attempt in task.execution_attempts.iter_mut() {
        if attempt.retired_at.is_some() || !selector(attempt) {
            continue;
        }
        attempt.retired_at = Some(now_ms);
        attempt.closed_at.get_or_insert(now_ms);
        retired.push(attempt.id.clone());
    }
    retired
}

fn recovered_status(outcome: &Option<TaskStatus>, current: &TaskStatus) -> Option<TaskStatus> {
    match (current, outcome) {
        (TaskStatus::InReview, Some(TaskStatus::Pending)) => Some(TaskStatus::Pending),
        (TaskStatus::InReview, _) => None,
        (_, outcome) => outcome.clone(),
    }
}

fn owner_is_stale(task: &Task, live_tab: impl Fn(&str) -> bool) -> bool {
    task.interruption
        .owner_tab_id
        .as_deref()
        .is_none_or(|tab_id| !live_tab(tab_id))
}

impl PlanStore {
    pub fn retire_stale_task_owner(
        &self,
        plan_id: &str,
        task_id: &str,
        outcome: Option<TaskStatus>,
        live_tab: impl Fn(&str) -> bool,
        now_ms: u64,
    ) -> Result<StaleOwnerRetirement, PlanError> {
        self.retire_stale_task_owner_with_label(plan_id, task_id, outcome, None, live_tab, now_ms)
    }

    pub fn retire_stale_task_owner_with_label(
        &self,
        plan_id: &str,
        task_id: &str,
        outcome: Option<TaskStatus>,
        status_label: Option<&str>,
        live_tab: impl Fn(&str) -> bool,
        now_ms: u64,
    ) -> Result<StaleOwnerRetirement, PlanError> {
        let status_label = status_label.unwrap_or(OWNER_RETIRED_LABEL);
        self.with_task_rows(
            plan_id,
            task_id,
            super::store_db::hot_write::TaskHistory::LiveAndReported,
            now_ms,
            |edit| {
                let task = &mut edit.task;
                if !owner_is_stale(task, &live_tab) {
                    return Err(PlanError::Io(
                        "task owner tab is still live; recovery is only for a tab-gone owner"
                            .into(),
                    ));
                }
                let previous_owner_tab_id = task.interruption.owner_tab_id.clone();
                let nothing_live = task
                    .execution_attempts
                    .iter()
                    .all(|attempt| attempt.closed_at.is_some());
                if nothing_live
                    && previous_owner_tab_id.is_none()
                    && task.interruption.owner_session_id.is_none()
                    && recovered_status(&outcome, &task.status)
                        .is_none_or(|status| task.status == status)
                    && task.status_label.as_deref() == Some(status_label)
                {
                    return Ok(StaleOwnerRetirement {
                        retired_attempt_ids: Vec::new(),
                        owner_epoch: task.interruption.owner_epoch,
                        previous_owner_tab_id,
                        applied: false,
                    });
                }
                let retired = retire_attempts(task, now_ms, |attempt| attempt.closed_at.is_none());
                let owner_epoch =
                    super::interruption::next_owner_epoch(task.interruption.owner_epoch)?;
                task.interruption.owner_epoch = owner_epoch;
                task.interruption.owner_tab_id = None;
                task.interruption.owner_session_id = None;
                task.interruption.handoff_required = false;
                task.assigned_tab_id = None;
                task.lane.tab_id = None;
                task.taken_by_role = None;
                let recovered = recovered_status(&outcome, &task.status);
                let recovered_review_to_pending = task.status == TaskStatus::InReview
                    && recovered.as_ref() == Some(&TaskStatus::Pending);
                if let Some(status) = recovered {
                    if task.status != status {
                        task.status_changed_at = now_ms;
                    }
                    task.status = status;
                }
                if recovered_review_to_pending {
                    super::qa::normalize_task_review_fields(task);
                    edit.review_credit = 1;
                }
                task.status_label = Some(status_label.into());
                task.updated_at = now_ms;
                Ok(StaleOwnerRetirement {
                    retired_attempt_ids: retired,
                    owner_epoch,
                    previous_owner_tab_id,
                    applied: true,
                })
            },
        )
    }
}

#[cfg(test)]
mod tests;
