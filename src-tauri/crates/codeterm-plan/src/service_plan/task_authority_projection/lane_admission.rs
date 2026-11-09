use crate::plan::TaskExecutionIntent;
use crate::service_plan::{PlanError, PlanStore};

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskLaneOwnerRead {
    pub task_id: String,
    pub worktree_path: String,
}

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskLaneAdmissionRead {
    pub plan_id: String,
    pub task_id: String,
    pub execution_intent: TaskExecutionIntent,
    pub canonical_path: String,
    pub foreign_lanes: Vec<TaskLaneOwnerRead>,
}

impl PlanStore {
    pub fn task_lane_admission_projection(
        &self,
        plan: &str,
        task: &str,
    ) -> Result<TaskLaneAdmissionRead, PlanError> {
        self.database.task_lane_admission_projection(plan, task)
    }
}
