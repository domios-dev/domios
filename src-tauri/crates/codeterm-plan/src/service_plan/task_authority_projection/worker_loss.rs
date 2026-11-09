use crate::service_plan::{PlanError, PlanStore};

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkerLossReplacementRead {
    pub attempt_id: String,
    pub executor_tab_id: String,
}

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkerLossRecoveryRead {
    pub latest_attempt_id: Option<String>,
    pub replacement: Option<WorkerLossReplacementRead>,
}

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkerLossRead {
    pub plan_id: String,
    pub task_id: String,
    pub task_name: String,
    pub orchestrator_tab_id: Option<String>,
    pub attempt_id: Option<String>,
    pub worktree_path: Option<String>,
    pub branch: Option<String>,
    pub latest_attempt_id: Option<String>,
    pub replacement: Option<WorkerLossReplacementRead>,
}

impl PlanStore {
    pub fn worker_loss_recovery_projection(
        &self,
        plan: &str,
        task: &str,
        tab: &str,
    ) -> Result<Option<WorkerLossRecoveryRead>, PlanError> {
        self.database
            .worker_loss_recovery_projection(plan, task, tab)
    }

    pub fn worker_loss_assignments_projection(
        &self,
        tab: &str,
    ) -> Result<Vec<WorkerLossRead>, PlanError> {
        let tab = tab.trim();
        if tab.is_empty() {
            return Ok(Vec::new());
        }
        self.database.worker_loss_projection(None, None, tab, true)
    }

    pub fn worker_loss_task_projection(
        &self,
        plan: &str,
        task: &str,
        tab: &str,
    ) -> Result<Option<WorkerLossRead>, PlanError> {
        self.database
            .worker_loss_projection(Some(plan), Some(task), tab, false)
            .map(|rows| rows.into_iter().next())
    }
}
