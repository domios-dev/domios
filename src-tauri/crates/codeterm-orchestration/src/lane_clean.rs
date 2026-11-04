use std::collections::HashSet;
use std::process::Command;

use serde::Serialize;

mod inventory;
use inventory::Inventory;

use crate::team::close::{self, TeamCloseLane};
use codeterm_service_core::plan::{Plan, Task, TaskStatus};
use codeterm_service_core::service::lane_removal::{find_lane_holder, LiveTabCwd};
use codeterm_service_core::service::plan::TaskLaneUpdate;
use codeterm_service_core::service::ServiceContext;

#[derive(Debug, Clone, Serialize)]
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
    close::lane_hold_reason(lane).or_else(|| {
        lane.branch.as_deref().and_then(|branch| {
            let reference = format!("refs/heads/{branch}");
            git(
                &lane.project_path,
                &["show-ref", "--verify", "--quiet", &reference],
            )
            .ok()
            .and_then(|_| close::merged_refusal(&lane.project_path, &reference, "HEAD"))
        })
    })
}

fn preflight_with_inventory(
    lane: &TeamCloseLane,
    tabs: &[LiveTabCwd],
    inventory: &Inventory,
) -> Option<String> {
    if let Some(holder) = lane
        .worktree_path
        .as_deref()
        .and_then(|path| find_lane_holder(path, tabs))
    {
        return Some(format!("occupied by tab {} ({})", holder.id, holder.title));
    }
    close::lane_hold_reason_with_inventory(
        lane,
        Some((&inventory.worktrees, &inventory.refs)),
        |cwd, reference, destination| inventory.merged_refusal(cwd, reference, destination),
    )
    .or_else(|| {
        lane.branch
            .as_deref()
            .filter(|branch| inventory.refs.contains_key(&format!("refs/heads/{branch}")))
            .and_then(|branch| {
                inventory.merged_refusal(
                    &lane.project_path,
                    &format!("refs/heads/{branch}"),
                    "HEAD",
                )
            })
    })
}

pub fn git(cwd: &str, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new("git");
    codeterm_service_core::process_util::no_window_std(&mut command);
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

#[cfg(test)]
pub fn remote_tips(lane: &TeamCloseLane, online: bool) -> Result<Vec<(String, String)>, String> {
    Inventory::load(&lane.project_path, online)?.remote_tips(lane)
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

#[cfg(test)]
pub fn run_lane_with(
    lane: &TeamCloseLane,
    confirm: bool,
    snapshot: impl Fn() -> (Vec<LiveTabCwd>, Option<String>),
) -> LaneCleanRow {
    let inventory = Inventory::load(&lane.project_path, confirm).unwrap();
    run_lane_with_inventory(lane, confirm, &inventory, snapshot)
}

fn run_lane_with_inventory(
    lane: &TeamCloseLane,
    confirm: bool,
    inventory: &Inventory,
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
    if let Some(reason) = refusal {
        row.reason = reason;
        return row;
    }
    if inventory.absent(lane) {
        row.action = "absent";
        row.reason = "lane worktree and branches already removed".into();
        return row;
    }
    if let Some(reason) = preflight_with_inventory(lane, &tabs, inventory) {
        row.reason = reason;
        return row;
    }
    let tips = match inventory.remote_tips(lane) {
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
    clean_plan_lanes_with_progress(svc, plan, confirm, fetch, |_, _| {})
}

pub fn clean_plan_lanes_with_progress(
    svc: &ServiceContext,
    plan: &Plan,
    confirm: bool,
    fetch: bool,
    mut progress: impl FnMut(&LaneCleanRow, bool),
) -> Result<Vec<LaneCleanRow>, String> {
    let started = std::time::Instant::now();
    let project = if plan.canonical_project_path.trim().is_empty() {
        &plan.project_path
    } else {
        &plan.canonical_project_path
    };
    if fetch {
        git(project, &["fetch", "--all", "--prune"])?;
    }
    let mut rows = Vec::new();
    let remote_tasks: HashSet<_> = plan
        .tasks
        .iter()
        .filter(|task| {
            task.execution_attempts
                .iter()
                .any(|attempt| attempt.remote_worktree_path.is_some())
        })
        .map(|task| task.id.as_str())
        .collect();
    let lanes = cleanup_inventory(plan);
    let lanes_were_empty = lanes.is_empty();
    let inventory = if lanes.is_empty() {
        Inventory::default()
    } else {
        Inventory::load(project, confirm)?
    };
    log::debug!(
        "[lane-clean] inventory: {} lanes in {:?}",
        lanes.len(),
        started.elapsed()
    );
    for lane in lanes {
        let lane_started = std::time::Instant::now();
        if inventory.absent(&lane) && !remote_tasks.contains(lane.task_id.as_str()) {
            let row = LaneCleanRow {
                task_id: lane.task_id.clone(),
                worktree: lane.worktree_path.clone(),
                branch: lane.branch.clone(),
                action: "absent",
                reason: "lane worktree and branches already removed".into(),
            };
            progress(&row, true);
            rows.push(row);
            continue;
        }
        progress(
            &LaneCleanRow {
                task_id: lane.task_id.clone(),
                worktree: lane.worktree_path.clone(),
                branch: lane.branch.clone(),
                action: "checking",
                reason: "assessing lane safety".into(),
            },
            false,
        );
        let refreshed = if confirm {
            Some(
                svc.plan_store
                    .get(&plan.id)
                    .ok_or_else(|| "plan disappeared during cleanup".to_string())?,
            )
        } else {
            None
        };
        let current = refreshed.as_ref().unwrap_or(plan);
        let task = current
            .tasks
            .iter()
            .find(|task| task.id == lane.task_id)
            .ok_or_else(|| "task disappeared during cleanup".to_string())?;
        let mut row = run_lane_with_inventory(&lane, confirm, &inventory, || {
            let live = svc
                .mux
                .read()
                .all_tabs()
                .map(|tab| tab.id.clone())
                .collect();
            let refreshed = confirm.then(|| svc.plan_store.get(&plan.id));
            let current = match &refreshed {
                Some(current) => current.as_ref(),
                None => Some(plan),
            };
            let refusal = match current {
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
                                .is_some_and(|(a, b)| {
                                    codeterm_service_core::service::worker_lane::same_path(a, b)
                                })
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
                codeterm_service_core::service::lane_removal::finalize_lane_removal(
                    &svc.plan_store,
                    svc.ext::<codeterm_service_core::repowatch::RepoWatchRegistryHandle>(),
                    &codeterm_service_core::service::lane_removal::LaneRemovalOutcome {
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
        log::debug!(
            "[lane-clean] {} {} in {:?}",
            lane.task_id,
            row.action,
            lane_started.elapsed()
        );
        progress(&row, true);
        rows.push(row);
    }
    let residues = if confirm || lanes_were_empty {
        codeterm_service_core::service::lane_removal::residue::inventory(
            project,
            &svc.live_tab_cwds(),
        )?
    } else {
        codeterm_service_core::service::lane_removal::residue::inventory_with_worktrees(
            project,
            &svc.live_tab_cwds(),
            &inventory.worktree_paths(),
        )?
    };
    for residue in residues {
        let mut row = LaneCleanRow {
            task_id: String::new(),
            worktree: Some(residue.path.display().to_string()),
            branch: None,
            action: "kept",
            reason: residue
                .refusal
                .clone()
                .unwrap_or_else(|| "unregistered artifact residue".into()),
        };
        if residue.refusal.is_none() {
            row.action = "remove";
            if confirm {
                match codeterm_service_core::service::lane_removal::residue::remove(
                    project,
                    &residue.path,
                    &svc.live_tab_cwds(),
                ) {
                    Ok(()) => row.action = "removed",
                    Err(reason) => {
                        row.action = "failed";
                        row.reason = reason;
                    }
                }
            }
        }
        progress(&row, true);
        rows.push(row);
    }
    log::debug!("[lane-clean] total: {:?}", started.elapsed());
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
