use super::{PlanError, PlanStore};
use crate::plan::{PlanMode, PlanningState, TaskFiles};

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

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PlanTaskAdmissionRead {
    pub mode: PlanMode,
    pub planning_state: PlanningState,
}

impl PlanTaskAdmissionRead {
    pub fn execution_tasks_unlocked(&self) -> bool {
        !matches!(self.mode, PlanMode::Complete | PlanMode::Paused)
            && (self.planning_state.allows_execution_tasks()
                || matches!(self.mode, PlanMode::Approved | PlanMode::Execution))
    }
}

pub(super) struct TabAttemptOwnerRead {
    pub plan_id: String,
    pub task_id: String,
    pub attempt_id: Option<String>,
    pub assigned_tab: Option<String>,
    pub competition_group: Option<String>,
    pub session_id: Option<String>,
    pub nonce: Option<String>,
}

impl PlanStore {
    pub fn task_name_projection(&self, task_ref: &str) -> Result<String, PlanError> {
        self.database.task_name_projection(task_ref)
    }

    pub fn task_authority_target_projection(
        &self,
        task_ref: &str,
    ) -> Result<super::TaskBindingRead, PlanError> {
        self.database.task_authority_target_projection(task_ref)
    }

    pub fn plan_task_admission_projection(
        &self,
        plan_id: &str,
    ) -> Result<PlanTaskAdmissionRead, PlanError> {
        self.database.plan_task_admission_projection(plan_id)
    }

    pub fn unplanned_task_section_projection(
        &self,
        plan_id: &str,
        parent_task_id: Option<&str>,
    ) -> Result<Option<String>, PlanError> {
        self.database
            .unplanned_task_section_projection(plan_id, parent_task_id)
    }

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

pub(super) mod worker_transfer;

pub(super) mod startup;
