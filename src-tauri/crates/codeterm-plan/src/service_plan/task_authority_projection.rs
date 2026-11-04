use super::{PlanError, PlanStore};
use crate::plan::TaskFiles;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TaskOwnerEpochRead {
    pub plan_id: String,
    pub task_id: String,
    pub owner_epoch: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_tab_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_session_id: Option<String>,
    pub handoff_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TaskContextFilesRead {
    pub plan_id: String,
    pub task_id: String,
    pub files: TaskFiles,
}

impl PlanStore {
    pub fn task_context_files_projection(
        &self,
        task_ref: &str,
    ) -> Result<TaskContextFilesRead, PlanError> {
        self.database.task_context_files_projection(task_ref)
    }

    pub fn task_owner_epoch_projection(
        &self,
        task_ref: &str,
    ) -> Result<TaskOwnerEpochRead, PlanError> {
        self.database.task_owner_epoch_projection(task_ref)
    }
}
