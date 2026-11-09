use crate::plan::InterruptionCheckpoint;
use crate::service_plan::{PlanError, PlanStore};

#[derive(Debug, serde::Serialize)]
pub struct TaskCheckpointAdoptionRead {
    pub plan_id: String,
    pub checkpoint: Option<InterruptionCheckpoint>,
}

impl PlanStore {
    pub fn task_checkpoint_adoption_projection(
        &self,
        task_ref: &str,
    ) -> Result<TaskCheckpointAdoptionRead, PlanError> {
        self.database.task_checkpoint_adoption_projection(task_ref)
    }
}
