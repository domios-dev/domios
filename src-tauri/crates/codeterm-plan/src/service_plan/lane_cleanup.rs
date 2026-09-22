use std::path::Path;
use std::process::Command;

use super::{PlanStore, TaskLaneUpdate};
use crate::plan::Plan;
use codeterm_util::process_util;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaneCleanupAction {
    RemoveWorktree(String),
    DeleteBranch(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaneCleanupDecision {
    Noop,
    Clean(Vec<LaneCleanupAction>),
    Refuse(String),
}

pub struct LaneCleanupInput<'a> {
    pub tab_id: Option<&'a str>,
    pub tab_live: bool,
    pub worktree_path: Option<&'a str>,
    pub worktree_exists: bool,
    pub branch: Option<&'a str>,
    pub branch_exists: bool,
    pub integration_target: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaneCleanupOutcome {
    Noop,
    Cleaned,
    Refused(String),
    Partial(String),
}

impl LaneCleanupOutcome {
    pub fn status(&self) -> &'static str {
        match self {
            LaneCleanupOutcome::Noop => "noop",
            LaneCleanupOutcome::Cleaned => "cleaned",
            LaneCleanupOutcome::Refused(_) => "refused",
            LaneCleanupOutcome::Partial(_) => "partial",
        }
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            LaneCleanupOutcome::Refused(r) | LaneCleanupOutcome::Partial(r) => Some(r.as_str()),
            _ => None,
        }
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|v| !v.is_empty())
}

// Branch deletion is irreversible -> gated on the merge check; worktree removal never forced.
pub fn decide_lane_cleanup<M>(
    input: &LaneCleanupInput<'_>,
    mut branch_merged: M,
) -> LaneCleanupDecision
where
    M: FnMut(&str, &str) -> bool,
{
    let worktree = non_empty(input.worktree_path);
    let branch = non_empty(input.branch);
    if worktree.is_none() && branch.is_none() {
        return LaneCleanupDecision::Noop;
    }
    if input.tab_live {
        let tab = input.tab_id.unwrap_or("worker");
        return LaneCleanupDecision::Refuse(format!(
            "worker tab {tab} is still live; close it before cleanup"
        ));
    }
    if input.branch_exists {
        if let Some(branch) = branch {
            let Some(target) = non_empty(input.integration_target) else {
                return LaneCleanupDecision::Refuse(format!(
                    "branch {branch} has no known integration target; leaving lane intact"
                ));
            };
            if !branch_merged(branch, target) {
                return LaneCleanupDecision::Refuse(format!(
                    "branch {branch} is not merged into {target}; leaving lane intact"
                ));
            }
        }
    }
    let mut actions = Vec::new();
    if input.worktree_exists {
        if let Some(worktree) = worktree {
            actions.push(LaneCleanupAction::RemoveWorktree(worktree.to_string()));
        }
    }
    if input.branch_exists {
        if let Some(branch) = branch {
            actions.push(LaneCleanupAction::DeleteBranch(branch.to_string()));
        }
    }
    LaneCleanupDecision::Clean(actions)
}

pub fn try_cleanup_task_lane<F>(
    store: &PlanStore,
    plan: &Plan,
    task_id: &str,
    mut tab_live: F,
    now_ms: u64,
) -> LaneCleanupOutcome
where
    F: FnMut(&str) -> bool,
{
    let Some(task) = plan.tasks.iter().find(|t| t.id == task_id) else {
        return LaneCleanupOutcome::Noop;
    };
    let already_done = task
        .lane
        .cleanup_status
        .as_deref()
        .is_some_and(|s| s.eq_ignore_ascii_case("done"));
    let tab_id = task
        .lane
        .tab_id
        .as_deref()
        .or(task.assigned_tab_id.as_deref());
    let project_path = plan_project_path(plan);
    let worktree_path = non_empty(task.lane.worktree_path.as_deref());
    let branch = non_empty(task.lane.branch.as_deref());
    let has_lane_references = tab_id.is_some()
        || worktree_path.is_some()
        || branch.is_some()
        || task.lane.workspace_id.is_some()
        || task.lane.workspace_name.is_some();
    if !has_lane_references {
        return LaneCleanupOutcome::Noop;
    }

    if tab_id.is_some_and(&mut tab_live) {
        return LaneCleanupOutcome::Refused(format!(
            "worker tab {} is still live; close it before cleanup",
            tab_id.unwrap_or("worker")
        ));
    }

    let worktree_exists = worktree_path.is_some_and(|path| Path::new(path).exists());
    let branch_is_present = branch.is_some_and(|name| branch_exists(project_path, name));
    let mut stale_references = TaskLaneUpdate::default();
    if already_done {
        stale_references.cleanup_status = Some(None);
    }
    if task.lane.tab_id.is_some() || task.assigned_tab_id.is_some() {
        stale_references.tab_id = Some(None);
    }
    if !worktree_exists {
        if worktree_path.is_some() {
            stale_references.worktree_path = Some(None);
        }
        if task.lane.workspace_id.is_some() {
            stale_references.workspace_id = Some(None);
        }
        if task.lane.workspace_name.is_some() {
            stale_references.workspace_name = Some(None);
        }
    }
    if branch.is_some() && !branch_is_present {
        stale_references.branch = Some(None);
    }
    let stale_references_changed = !stale_references.is_empty();
    if let Err(error) = update_task_lane(store, &plan.id, task_id, stale_references, now_ms) {
        return LaneCleanupOutcome::Partial(error);
    }

    let worktree_to_remove = worktree_path.filter(|_| worktree_exists);
    let branch_to_delete = branch.filter(|_| branch_is_present);
    if worktree_to_remove.is_none() && branch_to_delete.is_none() {
        return match update_task_lane(
            store,
            &plan.id,
            task_id,
            TaskLaneUpdate {
                cleanup_status: Some(Some("done".into())),
                ..Default::default()
            },
            now_ms,
        ) {
            Ok(()) => LaneCleanupOutcome::Cleaned,
            Err(error) => LaneCleanupOutcome::Partial(error),
        };
    }

    let integration_target = task
        .lane
        .destination
        .as_deref()
        .or(task.lane.base.as_deref())
        .or_else(|| plan_integration_fallback(plan));

    let decision = decide_lane_cleanup(
        &LaneCleanupInput {
            tab_id,
            tab_live: false,
            worktree_path: worktree_to_remove,
            worktree_exists: worktree_to_remove.is_some(),
            branch: branch_to_delete,
            branch_exists: branch_to_delete.is_some(),
            integration_target,
        },
        |branch, target| branch_is_merged(project_path, branch, target),
    );

    match decision {
        LaneCleanupDecision::Noop => LaneCleanupOutcome::Noop,
        LaneCleanupDecision::Refuse(reason) if stale_references_changed => {
            LaneCleanupOutcome::Partial(reason)
        }
        LaneCleanupDecision::Refuse(reason) => LaneCleanupOutcome::Refused(reason),
        LaneCleanupDecision::Clean(actions) => {
            for action in &actions {
                if let Err(error) = run_action(project_path, action) {
                    return LaneCleanupOutcome::Partial(error);
                }
                let update = match action {
                    LaneCleanupAction::RemoveWorktree(_) => TaskLaneUpdate {
                        workspace_id: Some(None),
                        workspace_name: Some(None),
                        worktree_path: Some(None),
                        ..Default::default()
                    },
                    LaneCleanupAction::DeleteBranch(_) => TaskLaneUpdate {
                        branch: Some(None),
                        ..Default::default()
                    },
                };
                if let Err(error) = update_task_lane(store, &plan.id, task_id, update, now_ms) {
                    return LaneCleanupOutcome::Partial(error);
                }
            }
            match update_task_lane(
                store,
                &plan.id,
                task_id,
                TaskLaneUpdate {
                    cleanup_status: Some(Some("done".into())),
                    ..Default::default()
                },
                now_ms,
            ) {
                Ok(()) => LaneCleanupOutcome::Cleaned,
                Err(error) => LaneCleanupOutcome::Partial(error),
            }
        }
    }
}

fn update_task_lane(
    store: &PlanStore,
    plan_id: &str,
    task_id: &str,
    update: TaskLaneUpdate,
    now_ms: u64,
) -> Result<(), String> {
    if update.is_empty() {
        return Ok(());
    }
    store
        .update_task_lane(plan_id, task_id, update, now_ms)
        .map_err(|error| format!("task lane metadata update failed: {error}"))
}

// Lanes in a plan share a base; borrow a sibling's target when this lane omits it.
fn plan_integration_fallback(plan: &Plan) -> Option<&str> {
    plan.tasks.iter().find_map(|task| {
        non_empty(task.lane.destination.as_deref()).or_else(|| non_empty(task.lane.base.as_deref()))
    })
}

fn plan_project_path(plan: &Plan) -> &str {
    if plan.canonical_project_path.trim().is_empty() {
        &plan.project_path
    } else {
        &plan.canonical_project_path
    }
}

fn branch_is_merged(project_path: &str, branch: &str, target: &str) -> bool {
    git_ok(
        project_path,
        &["merge-base", "--is-ancestor", branch, target],
    )
}

fn branch_exists(project_path: &str, branch: &str) -> bool {
    let reference = format!("refs/heads/{branch}");
    git_ok(
        project_path,
        &["show-ref", "--verify", "--quiet", &reference],
    )
}

fn run_action(project_path: &str, action: &LaneCleanupAction) -> Result<(), String> {
    match action {
        LaneCleanupAction::RemoveWorktree(path) => {
            if git_ok(project_path, &["worktree", "remove", path]) {
                Ok(())
            } else {
                Err(format!(
                    "worktree {path} could not be removed (dirty or locked); leaving lane intact"
                ))
            }
        }
        LaneCleanupAction::DeleteBranch(branch) => {
            if git_ok(project_path, &["branch", "-D", branch]) {
                Ok(())
            } else {
                Err(format!("branch {branch} could not be deleted"))
            }
        }
    }
}

fn git_ok(project_path: &str, args: &[&str]) -> bool {
    let mut cmd = Command::new("git");
    process_util::no_window_std(&mut cmd);
    cmd.arg("-C")
        .arg(project_path)
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests;
