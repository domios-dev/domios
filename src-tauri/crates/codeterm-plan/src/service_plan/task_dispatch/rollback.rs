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

/// The same current-attempt fence startup deadlines and acknowledgement use. An
/// attempt that was never recorded owns the task only while nothing else is live.
fn rollback_owns_task(task: &crate::plan::Task, tab_id: &str, attempt_id: &str) -> bool {
    if task
        .execution_attempts
        .iter()
        .any(|attempt| attempt.id == attempt_id)
    {
        return startup_attempt(task, tab_id, attempt_id).is_some();
    }
    task.execution_attempts
        .iter()
        .all(|attempt| attempt.closed_at.is_some())
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
        plan_id: &str,
        snapshot: &crate::plan::Task,
        plan_mode: crate::plan::PlanMode,
        tab_id: &str,
        attempt_id: &str,
        now_ms: u64,
    ) -> Result<DispatchRollback, PlanError> {
        let task_id = snapshot.id.clone();
        let current = self.read_task_for_verification(plan_id, &task_id)?;
        if !rollback_owns_task(&current, tab_id, attempt_id) {
            return Ok(superseded(&current));
        }
        let mut outcome = None;
        let mut restored_task = None;
        let write = self.with_plan(plan_id, now_ms, |plan| {
            let task = crate::plan::task_key::find_task_mut(&mut plan.tasks, &task_id)
                .ok_or(PlanError::TaskNotFound)?;
            if !rollback_owns_task(task, tab_id, attempt_id) {
                outcome = Some(superseded(task));
                return Ok(());
            }
            let failed_attempt = task
                .execution_attempts
                .iter()
                .find(|attempt| attempt.id == attempt_id)
                .cloned();
            let mut restored = snapshot.clone();
            if let Some(mut failed_attempt) = failed_attempt {
                failed_attempt.closed_at = Some(now_ms);
                restored.lane.destination = task.lane.destination.clone();
                restored.lane.cleanup_status = task.lane.cleanup_status.clone();
                restored.lane.cleanup_hints = task.lane.cleanup_hints.clone();
                failed_attempt.destination = restored.lane.destination.clone();
                restored.execution_attempts.push(failed_attempt);
                restored.updated_at = now_ms;
            }
            restored_task = Some(restored.clone());
            *task = restored;
            if snapshot.status == crate::plan::TaskStatus::Blocked {
                super::unblock::annotate_blocked_unblock_rollback(plan, &task_id, attempt_id);
            }
            plan.mode = plan_mode.clone();
            outcome = Some(DispatchRollback::Restored);
            Ok(())
        });
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
                    && plan.mode == plan_mode
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
