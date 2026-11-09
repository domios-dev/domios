use crate::service_plan::state::input::StateSource;
use std::path::Path;

use crate::plan::cleanup_debt::{
    detect_cleanup_debt, render_cleanup_debt_hint, CleanupDebtInput, CleanupDebtTask,
};

fn plan_project_path(plan: &crate::service_plan::state::input::StateInput) -> &str {
    if plan.canonical_project_path.trim().is_empty() {
        &plan.project_path
    } else {
        &plan.canonical_project_path
    }
}

pub fn detect_plan_cleanup_debt<F>(plan: &impl StateSource, mut tab_live: F) -> Vec<CleanupDebtTask>
where
    F: FnMut(&str) -> bool,
{
    let input = plan.state_input();
    let plan = input.as_ref();
    plan.tasks
        .iter()
        .filter_map(|task| {
            let tab_id = task
                .lane
                .tab_id
                .as_deref()
                .or(task.assigned_tab_id.as_deref());
            let tab_live = tab_id.map(|tab| tab_live(tab)).unwrap_or(false);
            let worktree_path = task.lane.worktree_path.as_deref();
            let worktree_exists = worktree_path.is_some_and(|path| Path::new(path).exists());
            detect_cleanup_debt(&CleanupDebtInput {
                plan_id: &plan.id,
                task_id: &task.id,
                task_name: &task.name,
                task_status: task_status_label(&task.status),
                is_terminal: task.status.is_terminal(),
                cleanup_status: task.lane.cleanup_status.as_deref(),
                cleanup_hints: &task.lane.cleanup_hints,
                tab_id,
                tab_live,
                worktree_path,
                worktree_exists,
                branch: task.lane.branch.as_deref(),
                project_path: plan_project_path(plan),
            })
        })
        .collect()
}

pub fn cleanup_debt_hints(items: &[CleanupDebtTask]) -> Vec<String> {
    items.iter().map(render_cleanup_debt_hint).collect()
}

fn task_status_label(status: &crate::plan::TaskStatus) -> &'static str {
    match status {
        crate::plan::TaskStatus::Pending => "pending",
        crate::plan::TaskStatus::InProgress => "in_progress",
        crate::plan::TaskStatus::InReview => "in_review",
        crate::plan::TaskStatus::Blocked => "blocked",
        crate::plan::TaskStatus::Paused => "paused",
        crate::plan::TaskStatus::Done => "done",
        crate::plan::TaskStatus::Cancelled => "cancelled",
    }
}

#[cfg(test)]
mod tests;
