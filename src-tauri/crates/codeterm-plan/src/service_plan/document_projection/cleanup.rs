use super::{PlanError, PlanStore};
use crate::plan::cleanup_debt::{detect_cleanup_debt, CleanupDebtInput, CleanupDebtTask};

#[derive(serde::Deserialize)]
pub(in crate::service_plan) struct CleanupPlanRead {
    pub id: String,
    pub project_path: String,
    pub canonical_project_path: String,
    #[serde(default)]
    pub tasks: Vec<CleanupTaskRead>,
}

#[derive(serde::Deserialize)]
pub(in crate::service_plan) struct CleanupTaskRead {
    pub id: String,
    pub name: String,
    pub status: String,
    pub cleanup_status: Option<String>,
    pub cleanup_hints: Vec<String>,
    pub tab_id: Option<String>,
    pub worktree_path: Option<String>,
    pub branch: Option<String>,
}

impl PlanStore {
    pub fn cleanup_debt_projection(
        &self,
        id: &str,
        tab_live: impl FnMut(&str) -> bool,
    ) -> Result<Option<Vec<CleanupDebtTask>>, PlanError> {
        self.cleanup_debt_projection_for_task(id, None, tab_live)
    }

    pub fn task_cleanup_debt_projection(
        &self,
        id: &str,
        task: &str,
        tab_live: impl FnMut(&str) -> bool,
    ) -> Result<Option<Vec<CleanupDebtTask>>, PlanError> {
        self.cleanup_debt_projection_for_task(id, Some(task), tab_live)
    }

    fn cleanup_debt_projection_for_task(
        &self,
        id: &str,
        task: Option<&str>,
        mut tab_live: impl FnMut(&str) -> bool,
    ) -> Result<Option<Vec<CleanupDebtTask>>, PlanError> {
        let Some(plan) = self.database.cleanup_debt_input(id, task)? else {
            return Ok(None);
        };
        let project_path = if plan.canonical_project_path.trim().is_empty() {
            &plan.project_path
        } else {
            &plan.canonical_project_path
        };
        Ok(Some(
            plan.tasks
                .into_iter()
                .filter_map(|task| {
                    let tab_id = task.tab_id.as_deref();
                    let live = tab_id.is_some_and(&mut tab_live);
                    let worktree_path = task.worktree_path.as_deref();
                    let worktree_exists =
                        worktree_path.is_some_and(|path| std::path::Path::new(path).exists());
                    detect_cleanup_debt(&CleanupDebtInput {
                        plan_id: &plan.id,
                        task_id: &task.id,
                        task_name: &task.name,
                        task_status: &task.status,
                        is_terminal: true,
                        cleanup_status: task.cleanup_status.as_deref(),
                        cleanup_hints: &task.cleanup_hints,
                        tab_id,
                        tab_live: live,
                        worktree_path,
                        worktree_exists,
                        branch: task.branch.as_deref(),
                        project_path,
                    })
                })
                .collect(),
        ))
    }
}
