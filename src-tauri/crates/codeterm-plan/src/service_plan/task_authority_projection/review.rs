use crate::plan::WorkerPlacement;
use crate::service_plan::{PlanError, PlanStore};

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CanonicalWriterHoldRead {
    pub task_id: String,
    pub attempt_id: String,
    pub placement: WorkerPlacement,
}

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LaneIntegrationRead {
    pub plan_id: String,
    pub task_id: String,
    pub worktree_path: String,
    pub canonical_path: String,
    pub holder: CanonicalWriterHoldRead,
}

impl PlanStore {
    pub fn task_review_worker_projection(
        &self,
        plan: &str,
        task: &str,
    ) -> Result<Option<String>, PlanError> {
        self.database.task_review_worker_projection(plan, task)
    }

    pub fn task_lane_integration_projection(
        &self,
        plan: &str,
        task: &str,
    ) -> Result<Option<LaneIntegrationRead>, PlanError> {
        self.database.task_lane_integration_projection(plan, task)
    }
}
