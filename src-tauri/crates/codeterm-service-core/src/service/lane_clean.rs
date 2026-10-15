use std::collections::HashSet;
use std::process::Command;

use serde::Serialize;

use super::lane_removal::{find_lane_holder, LiveTabCwd};
use super::plan::TaskLaneUpdate;
use super::team::close::{self, TeamCloseLane};
use super::ServiceContext;
use crate::plan::{Plan, Task, TaskStatus};

#[derive(Debug, Serialize)]
pub struct LaneCleanRow {
    task_id: String,
    worktree: Option<String>,
    branch: Option<String>,
    action: &'static str,
    reason: String,
}

pub fn task_refusal(task: &Task, lane: &TeamCloseLane, live: &HashSet<String>) -> Option<String> {
    let remote = task
        .execution_attempts
        .iter()
        .any(|attempt| attempt.remote_worktree_path.is_some());
    let current = task.lane.worktree_path == lane.worktree_path && task.lane.branch == lane.branch;
    let assigned = current
        && task
            .assigned_tab_id
            .as_ref()
            .is_some_and(|id| live.contains(id));
    let lane_tab = current
        && task
            .lane
            .tab_id
            .as_ref()
            .is_some_and(|id| live.contains(id));
    let attempt_tab = task.execution_attempts.iter().any(|attempt| {
        attempt.worktree_path == lane.worktree_path
            && attempt.branch == lane.branch
            && live.contains(&attempt.executor_tab_id)
    });
    task_eligibility(&task.status, remote, assigned || lane_tab || attempt_tab).map(str::to_string)
}

pub fn task_eligibility(status: &TaskStatus, remote: bool, occupied: bool) -> Option<&'static str> {
    if remote {
        return Some("remote lane requires cleanup on its owning peer");
    }
    if !status.is_terminal() && *status != TaskStatus::InReview {
        return Some("task is neither terminal nor in review");
    }
    occupied.then_some("lane Worker tab is live")
}

pub fn preflight(lane: &TeamCloseLane, tabs: &[LiveTabCwd]) -> Option<String> {
    if let Some(holder) = lane
        .worktree_path
        .as_deref()
        .and_then(|path| find_lane_holder(path, tabs))
    {
        return Some(format!("occupied by tab {} ({})", holder.id, holder.title));
    }
    close::lane_hold_reason(lane)
}

pub fn git(cwd: &str, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new("git");
    crate::process_util::no_window_std(&mut command);
    let output = command
        .current_dir(cwd)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {}: {}",
            args[0],
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn remote_tips(lane: &TeamCloseLane, online: bool) -> Result<Vec<(String, String)>, String> {
    let Some(branch) = lane.branch.as_deref() else {
        return Ok(Vec::new());
    };
    let mut tips = Vec::new();
    for remote in git(&lane.project_path, &["remote"])?.lines() {
        let reference = if online {
            format!("refs/heads/{branch}")
        } else {
            format!("refs/remotes/{remote}/{branch}")
        };
        let output = if online {
            git(
                &lane.project_path,
                &["ls-remote", "--refs", remote, &reference],
            )?
        } else {
            git(
                &lane.project_path,
                &[
                    "for-each-ref",
                    "--format=%(objectname) %(refname)",
                    &reference,
                ],
            )?
        };
        for line in output.lines() {
            let mut fields = line.split_whitespace();
            let (Some(tip), Some(found)) = (fields.next(), fields.next()) else {
                continue;
            };
            if found != reference {
                continue;
            }
            if let Some(reason) = close::merged_refusal(
                &lane.project_path,
                tip,
                lane.destination.as_deref().unwrap_or("HEAD"),
            ) {
                return Err(format!("remote {remote}: {reason}"));
            }
            tips.push((remote.to_string(), tip.to_string()));
        }
    }
    Ok(tips)
}

pub fn delete_remote_tips(lane: &TeamCloseLane, tips: &[(String, String)]) -> Result<(), String> {
    let Some(branch) = lane.branch.as_deref() else {
        return Ok(());
    };
    for (remote, tip) in tips {
        let lease = format!("--force-with-lease=refs/heads/{branch}:{tip}");
        let deletion = format!(":refs/heads/{branch}");
        git(&lane.project_path, &["push", &lease, remote, &deletion])?;
    }
    Ok(())
}

#[cfg(test)]
fn run_lane(
    lane: &TeamCloseLane,
    confirm: bool,
    tabs: &[LiveTabCwd],
    refusal: Option<String>,
) -> LaneCleanRow {
    run_lane_with(lane, confirm, || (tabs.to_vec(), refusal.clone()))
}

pub fn run_lane_with(
    lane: &TeamCloseLane,
    confirm: bool,
    snapshot: impl Fn() -> (Vec<LiveTabCwd>, Option<String>),
) -> LaneCleanRow {
    let (tabs, refusal) = snapshot();
    let mut row = LaneCleanRow {
        task_id: lane.task_id.clone(),
        worktree: lane.worktree_path.clone(),
        branch: lane.branch.clone(),
        action: "kept",
        reason: String::new(),
    };
    if let Some(reason) = refusal.or_else(|| preflight(lane, &tabs)) {
        row.reason = reason;
        return row;
    }
    let tips = match remote_tips(lane, confirm) {
        Ok(tips) => tips,
        Err(reason) => {
            row.reason = reason;
            return row;
        }
    };
    if !confirm {
        row.action = "remove";
        row.reason = "merged, clean, unoccupied; remote tips rechecked on confirm".into();
        return row;
    }
    let (tabs, refusal) = snapshot();
    if let Some(reason) = refusal.or_else(|| preflight(lane, &tabs)) {
        row.reason = reason;
        return row;
    }
    match close::cleanup_lane_with_tabs(lane, &tabs).and_then(|()| delete_remote_tips(lane, &tips))
    {
        Ok(()) => {
            row.action = "removed";
            row.reason = "merged lane worktree and local/remote branches removed".into();
        }
        Err(reason) => {
            row.action = "failed";
            row.reason = format!("cleanup incomplete: {reason}");
        }
    }
    row
}

pub fn cleanup_inventory(plan: &Plan) -> Vec<TeamCloseLane> {
    let mut lanes: Vec<TeamCloseLane> = Vec::new();
    for mut lane in close::plan_cleanup_lanes(plan) {
        if lane.destination.is_none() {
            lane.destination = Some("HEAD".into());
        }
        if let Some(existing) = lanes.iter_mut().find(|existing| {
            existing.task_id == lane.task_id
                && existing.worktree_path == lane.worktree_path
                && existing.branch == lane.branch
        }) {
            if existing.destination != lane.destination {
                existing.hold_reason = Some("conflicting recorded lane destinations".into());
            }
        } else {
            lanes.push(lane);
        }
    }
    lanes
}

pub fn clean_plan_lanes(
    svc: &ServiceContext,
    plan: &Plan,
    confirm: bool,
    fetch: bool,
) -> Result<Vec<LaneCleanRow>, String> {
    let project = if plan.canonical_project_path.trim().is_empty() {
        &plan.project_path
    } else {
        &plan.canonical_project_path
    };
    if fetch {
        git(project, &["fetch", "--all", "--prune"])?;
    }
    let mut rows = Vec::new();
    let lanes = cleanup_inventory(plan);
    for lane in lanes {
        let current = svc
            .plan_store
            .get(&plan.id)
            .ok_or_else(|| "plan disappeared during cleanup".to_string())?;
        let task = current
            .tasks
            .iter()
            .find(|task| task.id == lane.task_id)
            .ok_or_else(|| "task disappeared during cleanup".to_string())?;
        let mut row = run_lane_with(&lane, confirm, || {
            let live = svc
                .mux
                .read()
                .all_tabs()
                .map(|tab| tab.id.clone())
                .collect();
            let refusal = match svc.plan_store.get(&plan.id) {
                None => Some("plan disappeared during cleanup".into()),
                Some(current) if !current.tasks.iter().any(|task| task.id == lane.task_id) => {
                    Some("task disappeared during cleanup".into())
                }
                Some(current) => current
                    .tasks
                    .iter()
                    .filter(|task| {
                        task.id == lane.task_id
                            || task
                                .lane
                                .worktree_path
                                .as_ref()
                                .zip(lane.worktree_path.as_ref())
                                .is_some_and(|(a, b)| super::worker_lane::same_path(a, b))
                            || task
                                .lane
                                .branch
                                .as_ref()
                                .zip(lane.branch.as_ref())
                                .is_some_and(|(a, b)| a == b)
                    })
                    .find_map(|task| task_refusal(task, &lane, &live)),
            };
            (svc.live_tab_cwds(), refusal)
        });
        if row.action == "removed" {
            let finalized = if let Some(path) = lane.worktree_path.as_deref() {
                super::lane_removal::finalize_lane_removal(
                    &svc.plan_store,
                    svc.ext::<crate::repowatch::RepoWatchRegistryHandle>(),
                    &super::lane_removal::LaneRemovalOutcome {
                        canonical_cwd: project.clone(),
                        worktree_path: path.into(),
                        branch: lane.branch.clone(),
                        worktree_already_removed: false,
                        branch_already_removed: false,
                    },
                )
            } else if task.lane.worktree_path.is_none() && task.lane.branch == lane.branch {
                svc.plan_store
                    .update_task_lane(
                        &plan.id,
                        &task.id,
                        TaskLaneUpdate {
                            tab_id: Some(None),
                            workspace_id: Some(None),
                            workspace_name: Some(None),
                            branch: Some(None),
                            cleanup_status: Some(Some("done".into())),
                            ..Default::default()
                        },
                        now_ms(),
                    )
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            } else {
                Ok(())
            };
            if let Err(reason) = finalized {
                row.action = "failed";
                row.reason =
                    format!("lane removed but cleanup bookkeeping is incomplete: {reason}");
            }
        }
        rows.push(row);
    }
    Ok(rows)
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests;
