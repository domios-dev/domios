use crate::plan::TaskLaneMetadata;
use crate::service_plan::{PlanError, PlanStore};

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct RuntimeSessionAttemptRead {
    pub session_id: Option<String>,
    pub session_binding_nonce: Option<String>,
    pub closed_at: Option<u64>,
    pub retired_at: Option<u64>,
}

#[derive(Debug, serde::Serialize)]
pub struct TaskRuntimeSessionRead {
    pub plan_id: String,
    pub owner_epoch: u64,
    pub attempt: Option<RuntimeSessionAttemptRead>,
    pub lane: Option<TaskLaneMetadata>,
}

impl PlanStore {
    pub fn task_runtime_session_projection(
        &self,
        task_ref: &str,
        attempt_id: &str,
        executor_tab_id: &str,
        include_lane: bool,
    ) -> Result<TaskRuntimeSessionRead, PlanError> {
        self.database.task_runtime_session_projection(
            task_ref,
            attempt_id,
            executor_tab_id,
            include_lane,
        )
    }
}
