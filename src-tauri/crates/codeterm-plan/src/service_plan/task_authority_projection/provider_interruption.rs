use crate::service_plan::{PlanError, PlanStore};

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProviderRecoveryRead {
    pub plan_id: String,
    pub task_id: String,
    pub owner_tab_id: String,
    pub owner_session_id: String,
    pub owner_epoch: u64,
}

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskInterruptionRead {
    pub plan_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub owner_session_id: String,
    pub owner_epoch: u64,
    pub repo_path: String,
    pub worktree_path: String,
    pub base_head: Option<String>,
    pub test_plan: Option<String>,
}

impl PlanStore {
    pub fn provider_exhaustion_tabs_projection(
        &self,
        provider: &str,
    ) -> Result<Vec<String>, PlanError> {
        self.database.provider_exhaustion_tabs_projection(provider)
    }

    pub fn provider_recovery_projection(
        &self,
        provider: &str,
    ) -> Result<Vec<ProviderRecoveryRead>, PlanError> {
        self.database.provider_recovery_projection(provider)
    }

    pub fn task_interruption_projection(
        &self,
        tab_id: &str,
    ) -> Result<Option<TaskInterruptionRead>, PlanError> {
        self.database.task_interruption_projection(tab_id)
    }
}
