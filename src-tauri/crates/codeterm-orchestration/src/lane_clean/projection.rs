use super::*;
use codeterm_service_core::service::plan::{LaneCleanupAttemptRead, LaneCleanupTaskRead};

pub(super) fn from_plan(plan: &Plan) -> LaneCleanupRead {
    LaneCleanupRead {
        id: plan.id.clone(),
        project_path: plan.project_path.clone(),
        canonical_project_path: plan.canonical_project_path.clone(),
        tasks: plan
            .tasks
            .iter()
            .map(|task| LaneCleanupTaskRead {
                id: task.id.clone(),
                status: task.status.clone(),
                assigned_tab_id: task.assigned_tab_id.clone(),
                lane: task.lane.clone(),
                remote: task
                    .execution_attempts
                    .iter()
                    .any(|attempt| attempt.remote_worktree_path.is_some()),
                execution_attempts: task
                    .execution_attempts
                    .iter()
                    .map(|attempt| LaneCleanupAttemptRead {
                        executor_tab_id: attempt.executor_tab_id.clone(),
                        worktree_path: attempt.worktree_path.clone(),
                        branch: attempt.branch.clone(),
                        base: attempt.base.clone(),
                        destination: attempt.destination.clone(),
                    })
                    .collect(),
            })
            .collect(),
    }
}

pub(super) fn task_refusal(
    task: &LaneCleanupTaskRead,
    lane: &TeamCloseLane,
    live: &HashSet<String>,
) -> Option<String> {
    let current = task.lane.worktree_path == lane.worktree_path && task.lane.branch == lane.branch;
    let assigned = current
        && task
            .assigned_tab_id
            .as_ref()
            .is_some_and(|tab| live.contains(tab));
    let lane_tab = current
        && task
            .lane
            .tab_id
            .as_ref()
            .is_some_and(|tab| live.contains(tab));
    let attempt_tab = task.execution_attempts.iter().any(|attempt| {
        attempt.worktree_path == lane.worktree_path
            && attempt.branch == lane.branch
            && live.contains(&attempt.executor_tab_id)
    });
    task_eligibility(
        &task.status,
        task.remote,
        assigned || lane_tab || attempt_tab,
    )
    .map(str::to_string)
}

pub(super) fn cleanup_inventory(plan: &LaneCleanupRead) -> Vec<TeamCloseLane> {
    let project = if plan.canonical_project_path.trim().is_empty() {
        &plan.project_path
    } else {
        &plan.canonical_project_path
    };
    let mut lanes = Vec::new();
    for task in &plan.tasks {
        close::push_lane(
            &mut lanes,
            &plan.id,
            &task.id,
            project,
            task.lane.worktree_path.as_deref(),
            task.lane.branch.as_deref(),
            task.lane.base.as_deref(),
            task.lane.destination.as_deref(),
        );
        for attempt in &task.execution_attempts {
            close::push_lane(
                &mut lanes,
                &plan.id,
                &task.id,
                project,
                attempt
                    .worktree_path
                    .as_deref()
                    .or(task.lane.worktree_path.as_deref()),
                attempt.branch.as_deref().or(task.lane.branch.as_deref()),
                attempt.base.as_deref().or(task.lane.base.as_deref()),
                attempt
                    .destination
                    .as_deref()
                    .or(task.lane.destination.as_deref()),
            );
        }
    }
    deduplicate_cleanup_inventory(lanes)
}

#[cfg(test)]
mod tests;
