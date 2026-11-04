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
    super::super::task_authority_projection::ensure_respawn_source_fields(
        &current.id,
        &current.executor_tab_id,
        current.closed_at.is_some(),
        current.retired_at.is_some(),
        source_attempt_id,
    )
}
