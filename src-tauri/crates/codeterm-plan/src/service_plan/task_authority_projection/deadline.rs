use crate::plan::TaskLaneMetadata;
use crate::service_plan::{PlanError, PlanStore};

impl PlanStore {
    pub fn startup_owner_epoch_projection(
        &self,
        plan: &str,
        task: &str,
        attempt: &str,
        executor: &str,
    ) -> Result<Option<u64>, PlanError> {
        self.database
            .startup_owner_epoch_projection(plan, task, attempt, executor)
    }

    pub fn lifecycle_provider_projection(
        &self,
        plan: &str,
        task: &str,
        attempt: &str,
    ) -> Result<Option<String>, PlanError> {
        self.database
            .lifecycle_provider_projection(plan, task, attempt)
    }

    pub fn task_lane_metadata_projection(
        &self,
        plan: &str,
        task: &str,
    ) -> Result<TaskLaneMetadata, PlanError> {
        self.database.task_lane_metadata_projection(plan, task)
    }
}
