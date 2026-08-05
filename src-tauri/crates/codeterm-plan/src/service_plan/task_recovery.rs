use crate::plan::{Task, TaskStatus};

use super::readback::pending_after_verified;
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
    outcome
        .clone()
        .filter(|_| *current != TaskStatus::InReview)
}

fn owner_is_stale(task: &Task, live_tab: impl Fn(&str) -> bool) -> bool {
    task.interruption
        .owner_tab_id
        .as_deref()
        .is_none_or(|tab_id| !live_tab(tab_id))
}

impl PlanStore {
    /// ONE `with_plan` transaction: attempts retired, dead owner cleared, epoch
    /// superseded, recovered status applied — never observable half-applied.
    pub fn retire_stale_task_owner(
        &self,
        plan_id: &str,
        task_id: &str,
        outcome: Option<TaskStatus>,
        live_tab: impl Fn(&str) -> bool,
        now_ms: u64,
    ) -> Result<StaleOwnerRetirement, PlanError> {
        let mut retirement = None;
        let write = self.with_plan(plan_id, now_ms, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id || task.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            if !owner_is_stale(task, &live_tab) {
                return Err(PlanError::Io(
                    "task owner tab is still live; recovery is only for a tab-gone owner".into(),
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
            {
                retirement = Some(StaleOwnerRetirement {
                    retired_attempt_ids: Vec::new(),
                    owner_epoch: task.interruption.owner_epoch,
                    previous_owner_tab_id,
                    applied: false,
                });
                return Ok(());
            }
            let retired = retire_attempts(task, now_ms, |attempt| attempt.closed_at.is_none());
            let owner_epoch = task.interruption.owner_epoch.saturating_add(1);
            task.interruption.owner_epoch = owner_epoch;
            task.interruption.owner_tab_id = None;
            task.interruption.owner_session_id = None;
            task.interruption.handoff_required = false;
            task.assigned_tab_id = None;
            task.lane.tab_id = None;
            task.taken_by_role = None;
            // `in_review` keeps its accept/bounce discipline: retirement still
            // lands atomically, but the verb's own validated transition decides.
            let recovered = recovered_status(&outcome, &task.status);
            if let Some(status) = recovered {
                if task.status != status {
                    task.status_changed_at = now_ms;
                }
                task.status = status;
            }
            task.status_label = Some(OWNER_RETIRED_LABEL.into());
            task.updated_at = now_ms;
            retirement = Some(StaleOwnerRetirement {
                retired_attempt_ids: retired,
                owner_epoch,
                previous_owner_tab_id,
                applied: true,
            });
            Ok(())
        });
        let retirement = retirement.ok_or_else(|| {
            Self::task_readback_error("task_owner_retire", "retirement was not applied")
        })?;
        if !retirement.applied {
            pending_after_verified(write, Ok(()), Some(task_id.to_string()))?;
            return Ok(retirement);
        }
        let verified = (|| {
            let task = self.read_task_for_verification(plan_id, task_id)?;
            let all_retired = retirement.retired_attempt_ids.iter().all(|attempt_id| {
                task.execution_attempts.iter().any(|attempt| {
                    attempt.id == *attempt_id
                        && attempt.retired_at.is_some()
                        && attempt.closed_at.is_some()
                })
            });
            let status_matches =
                recovered_status(&outcome, &task.status).is_none_or(|status| task.status == status);
            if !all_retired
                || !status_matches
                || task.interruption.owner_epoch != retirement.owner_epoch
                || task.interruption.owner_tab_id.is_some()
                || task.interruption.owner_session_id.is_some()
                || task.interruption.handoff_required
                || task.assigned_tab_id.is_some()
                || task.status_label.as_deref() != Some(OWNER_RETIRED_LABEL)
            {
                return Err(Self::task_readback_error(
                    "task_owner_retire",
                    format!("stale owner retirement did not match for {}", task.id),
                ));
            }
            Ok(())
        })();
        pending_after_verified(write, verified, Some(task_id.to_string()))?;
        Ok(retirement)
    }
}

#[cfg(test)]
mod tests;
