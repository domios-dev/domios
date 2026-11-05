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
        self.with_task_rows(
            &undo.plan_id,
            &undo.task_id,
            super::super::store_db::hot_write::TaskHistory::Selected(undo.row_attempt_ids()),
            now_ms,
            |edit| {
                let task = &mut edit.task;
                if undo.row_attempt_ids().iter().any(|id| {
                    task.execution_attempts
                        .iter()
                        .filter(|attempt| &attempt.id == id)
                        .count()
                        != 1
                }) {
                    return Err(PlanError::Io(
                        "dispatch rollback conflict: attempt row identity is ambiguous".into(),
                    ));
                }
                if !rollback_owns_task(task, tab_id, attempt_id) || !undo.owner_epoch_current(task)
                {
                    return Ok(superseded(task));
                }
                undo.apply(task);
                let failed_attempt = task
                    .execution_attempts
                    .iter_mut()
                    .find(|attempt| attempt.id == attempt_id)
                    .ok_or(PlanError::TaskNotFound)?;
                failed_attempt.closed_at = Some(now_ms);
                failed_attempt.destination = task.lane.destination.clone();
                task.updated_at = task.updated_at.max(now_ms);
                if undo.restores_blocked() && task.status == crate::plan::TaskStatus::Blocked {
                    edit.rollback_unblock = Some(attempt_id.into());
                }
                if undo.before_mode != undo.after_mode && edit.mode == undo.after_mode {
                    edit.mode = undo.before_mode.clone();
                }
                Ok(DispatchRollback::Restored)
            },
        )
    }
}

#[cfg(test)]
mod tests;
