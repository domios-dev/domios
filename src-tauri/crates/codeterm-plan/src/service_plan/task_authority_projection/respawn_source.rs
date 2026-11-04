use crate::service_plan::{PlanError, PlanStore};

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(in crate::service_plan) struct TaskRespawnSourceRead {
    pub attempt_id: String,
    pub executor_tab_id: String,
    pub closed: bool,
    pub retired: bool,
}

pub(in crate::service_plan) fn ensure_respawn_source_fields(
    attempt: &str,
    executor: &str,
    closed: bool,
    retired: bool,
    source: &str,
) -> Result<(), PlanError> {
    if attempt == source && (!retired || closed) {
        return Ok(());
    }
    Err(PlanError::RespawnAttemptNotCurrent {
        current_attempt_id: attempt.into(),
        tab_id: executor.into(),
    })
}

impl PlanStore {
    pub(crate) fn verify_respawn_source_projection(
        &self,
        plan: &str,
        task: &str,
        source: &str,
    ) -> Result<(), PlanError> {
        let current = self
            .database
            .task_respawn_source_projection(plan, task)?
            .ok_or(PlanError::TaskNotFound)?;
        ensure_respawn_source_fields(
            &current.attempt_id,
            &current.executor_tab_id,
            current.closed,
            current.retired,
            source,
        )
    }
}
