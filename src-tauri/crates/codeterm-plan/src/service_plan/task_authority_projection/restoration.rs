use crate::plan::TaskStatus;
use crate::service_plan::{PlanError, PlanStore};

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct TaskBindingRestoreRead {
    pub plan_id: String,
    pub task_id: String,
    pub team_id: Option<String>,
    pub attempt_count: u64,
    pub attempt_nonce: Option<String>,
}

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ReviewRestoreAttemptRead {
    pub id: String,
    pub executor_tab_id: String,
    pub session_binding_nonce: Option<String>,
}

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ReviewBindingRestoreRead {
    pub task_id: String,
    pub status: TaskStatus,
    pub owner_epoch: u64,
    pub attempts: Vec<ReviewRestoreAttemptRead>,
}

impl PlanStore {
    pub fn task_binding_restore_projection(
        &self,
        task_ref: &str,
        attempt_id: &str,
    ) -> Result<TaskBindingRestoreRead, PlanError> {
        self.database
            .task_binding_restore_projection(task_ref, attempt_id)
    }

    /// Returns at most two open attempts to distinguish absent, unique and ambiguous.
    pub fn review_binding_restore_projection(
        &self,
        plan_id: &str,
        task_ref: &str,
    ) -> Result<ReviewBindingRestoreRead, PlanError> {
        self.database
            .review_binding_restore_projection(plan_id, task_ref)
    }
}
