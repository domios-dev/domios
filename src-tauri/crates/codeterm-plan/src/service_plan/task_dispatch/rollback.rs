use super::super::readback::pending_after_verified;
use super::super::{PlanError, PlanStore};
use super::startup_attempt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchRollback {
    Restored,
    /// A newer attempt owns the task; the failed attempt stays closed history.
    Superseded {
        live_tab_id: Option<String>,
        live_worktree_path: Option<String>,
    },
}

/// Compensation uses the same current-attempt fence as startup deadlines and ACK.
fn rollback_owns_task(task: &crate::plan::Task, tab_id: &str, attempt_id: &str) -> bool {
    startup_attempt(task, tab_id, attempt_id).is_some()
}

fn superseded(task: &crate::plan::Task) -> DispatchRollback {
    DispatchRollback::Superseded {
        live_tab_id: task.assigned_tab_id.clone(),
        live_worktree_path: task.lane.worktree_path.clone(),
    }
}

impl PlanStore {
    pub fn roll_back_task_dispatch_attempt(
        &self,
        undo: &super::DispatchUndo,
        tab_id: &str,
        attempt_id: &str,
        now_ms: u64,
    ) -> Result<DispatchRollback, PlanError> {
        if undo.attempt_id != attempt_id {
            return Err(PlanError::InvalidPlanText(
                "dispatch rollback token does not match attempt".into(),
            ));
        }
        let plan_id = &undo.plan_id;
        let task_id = undo.task_id.clone();
        let mut outcome = None;
        let mut restored_task = None;
        let mut restored_mode = None;
        let write = self
            .with_plan_if_changed(plan_id, now_ms, |plan| {
                let task = crate::plan::task_key::find_task_mut(&mut plan.tasks, &task_id)
                    .ok_or(PlanError::TaskNotFound)?;
                if !rollback_owns_task(task, tab_id, attempt_id) || !undo.owner_epoch_current(task)
                {
                    outcome = Some(superseded(task));
                    return Ok(None);
                }
                let failed_index = task
                    .execution_attempts
                    .iter()
                    .position(|attempt| attempt.id == attempt_id)
                    .ok_or(PlanError::TaskNotFound)?;
                undo.apply(task);
                let mut failed_attempt = task.execution_attempts.remove(failed_index);
                failed_attempt.closed_at = Some(now_ms);
                failed_attempt.destination = task.lane.destination.clone();
                task.execution_attempts.push(failed_attempt);
                task.updated_at = task.updated_at.max(now_ms);
                restored_task = Some(task.clone());
                if undo.restores_blocked() && task.status == crate::plan::TaskStatus::Blocked {
                    super::unblock::annotate_blocked_unblock_rollback(plan, &task_id, attempt_id);
                }
                if undo.before_mode != undo.after_mode && plan.mode == undo.after_mode {
                    plan.mode = undo.before_mode.clone();
                }
                restored_mode = Some(plan.mode.clone());
                outcome = Some(DispatchRollback::Restored);
                Ok(Some(()))
            })
            .map(|_| ());
        let write = match write {
            Ok(()) => Ok(()),
            Err(error @ PlanError::WritePending { .. }) => Err(error),
            Err(error) => return Err(error),
        };
        let outcome = outcome.ok_or_else(|| {
            Self::task_readback_error("task_dispatch_restore", "rollback was not decided")
        })?;
        if outcome != DispatchRollback::Restored {
            pending_after_verified(write, Ok(()), Some(task_id))?;
            return Ok(outcome);
        }
        let verified = self
            .committed_plan_for(plan_id)
            .ok_or(PlanError::NotFound)
            .and_then(|plan| {
                let task = crate::plan::task_key::find_task(&plan.tasks, &task_id)
                    .ok_or(PlanError::TaskNotFound)?;
                if restored_task
                    .as_ref()
                    .is_some_and(|restored| task == restored)
                    && restored_mode.as_ref() == Some(&plan.mode)
                {
                    Ok(())
                } else {
                    Err(Self::task_readback_error(
                        "task_dispatch_restore",
                        format!("task snapshot did not match for {task_id}"),
                    ))
                }
            });
        pending_after_verified(write, verified, Some(task_id))?;
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests;
