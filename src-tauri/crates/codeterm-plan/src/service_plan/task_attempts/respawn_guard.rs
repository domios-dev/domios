use crate::plan::Task;

use super::super::PlanError;

pub(crate) fn ensure_respawn_source_current(
    task: &Task,
    source_attempt_id: &str,
) -> Result<(), PlanError> {
    let current = task
        .execution_attempts
        .last()
        .ok_or(PlanError::TaskNotFound)?;
    if current.id == source_attempt_id && current.retired_at.is_none() {
        return Ok(());
    }
    Err(PlanError::RespawnAttemptNotCurrent {
        current_attempt_id: current.id.clone(),
        tab_id: current.executor_tab_id.clone(),
    })
}
