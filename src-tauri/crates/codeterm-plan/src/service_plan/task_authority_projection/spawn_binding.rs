use crate::plan::TaskLaneMetadata;
use crate::service_plan::{PlanError, PlanStore};

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct TaskSpawnBindingRead {
    pub lane: TaskLaneMetadata,
    pub attempt: Option<SpawnAttemptBindingRead>,
}

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct SpawnAttemptBindingRead {
    pub id: String,
    pub executor_tab_id: String,
    pub session_id: Option<String>,
    pub session_binding_nonce: Option<String>,
    pub closed_at: Option<u64>,
    pub retired_at: Option<u64>,
}

impl PlanStore {
    pub fn task_spawn_binding_projection(
        &self,
        plan: &str,
        task: &str,
        attempt: &str,
    ) -> Result<TaskSpawnBindingRead, PlanError> {
        self.database
            .task_spawn_binding_projection(plan, task, attempt)
    }

    pub fn task_attempt_exists_projection(
        &self,
        plan: &str,
        task: &str,
        attempt: &str,
    ) -> Result<bool, PlanError> {
        self.database
            .task_attempt_exists_projection(plan, task, attempt)
    }
}
