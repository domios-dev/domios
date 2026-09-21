use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::{Command, Output};

use codeterm_types::team::Team;

use super::TeamMutationCaller;
use crate::multiplexer::state::{MultiplexerState, TeamCloseTransaction};
use crate::multiplexer::types::TabBacking;
use crate::plan::{Plan, TaskStatus};
use crate::process_util;
use crate::service::plan::{PlanStore, TaskLaneUpdate};

#[derive(Debug)]
pub(crate) struct CloseTeamCleanup {
    pub(super) member_tab_ids: Vec<String>,
    pub(super) chat_backings: Vec<(String, String)>,
    pub(super) pty_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TeamCloseLane {
    plan_id: String,
    task_id: String,
    project_path: String,
    worktree_path: Option<String>,
    branch: Option<String>,
    base: Option<String>,
    holds_work: bool,
}

pub(super) struct PreparedTeamClose {
    pub(super) member_tab_ids: Vec<String>,
}

struct StagedTeamClose {
    team: Team,
    member_tab_ids: Vec<String>,
    chat_backings: Vec<(String, String)>,
    pty_ids: Vec<String>,
    closed_roots: Vec<String>,
    transaction: TeamCloseTransaction,
}

pub(crate) fn close_team_with_state_change(
    mux: &mut MultiplexerState,
    events: &crate::api::events::EventBroadcaster,
    plan_store: &PlanStore,
    team_id: &str,
    caller: &TeamMutationCaller,
    now_ms: u64,
) -> Result<(Team, CloseTeamCleanup, Vec<String>), String> {
    close_team_with_state_change_using(
        mux,
        events,
        plan_store,
        team_id,
        caller,
        now_ms,
        |mux, tab_id| mux.close_tab_for_team_close(tab_id),
    )
}

pub(super) fn close_team_with_state_change_using<F>(
    mux: &mut MultiplexerState,
    events: &crate::api::events::EventBroadcaster,
    plan_store: &PlanStore,
    team_id: &str,
    caller: &TeamMutationCaller,
    now_ms: u64,
    mut close_member: F,
) -> Result<(Team, CloseTeamCleanup, Vec<String>), String>
where
    F: FnMut(&mut MultiplexerState, &str) -> Result<Vec<String>, String>,
{
    let initial = stage_team_close(mux, team_id, caller, &mut close_member)?;
    let lanes = team_close_lanes(
        &plan_store.list_full(),
        team_id,
        &initial.member_tab_ids,
    );
    let transaction = plan_store
        .close_team_task_attempts_transaction(team_id, &initial.member_tab_ids, now_ms)
        .map_err(|error| error.to_string())?;
    let changed_plan_ids = transaction.changed_plan_ids().to_vec();
    let staged = match stage_team_close(
        mux,
        team_id,
        caller,
        &mut |mux, tab_id| mux.close_tab_for_team_close(tab_id),
    ) {
        Ok(staged) if staged.member_tab_ids == initial.member_tab_ids => staged,
        Ok(_) => {
            plan_store
                .rollback_team_plan_archive(transaction)
                .map_err(|error| error.to_string())?;
            return Err("Team membership changed during close; retry the close".into());
        }
        Err(error) => {
            plan_store
                .rollback_team_plan_archive(transaction)
                .map_err(|rollback| format!("{error}; plan rollback failed: {rollback}"))?;
            return Err(error);
        }
    };
    let (team, cleanup) = commit_staged_team_close(mux, staged);
    crate::service::publish_mux_state_changed(events);
    finish_team_lane_cleanup(plan_store, &lanes, now_ms)?;
    Ok((team, cleanup, changed_plan_ids))
}

pub(super) fn prepare_team_close(
    mux: &MultiplexerState,
    team_id: &str,
    caller: &TeamMutationCaller,
) -> Result<PreparedTeamClose, String> {
    let staged = stage_team_close(mux, team_id, caller, &mut |mux, tab_id| {
        mux.close_tab_for_team_close(tab_id)
    })?;
    Ok(PreparedTeamClose {
        member_tab_ids: staged.member_tab_ids,
    })
}

pub(super) fn commit_prepared_team_close(
    mux: &mut MultiplexerState,
    team_id: &str,
    caller: &TeamMutationCaller,
    expected: &PreparedTeamClose,
) -> Result<(Team, CloseTeamCleanup), String> {
    let staged = stage_team_close(mux, team_id, caller, &mut |mux, tab_id| {
        mux.close_tab_for_team_close(tab_id)
    })?;
    if staged.member_tab_ids != expected.member_tab_ids {
        return Err("Team membership changed during close; retry the close".into());
    }
    let (team, cleanup) = commit_staged_team_close(mux, staged);
    Ok((team, cleanup))
}

pub(super) fn team_close_lanes(
    plans: &[Plan],
    team_id: &str,
    member_tab_ids: &[String],
) -> Vec<TeamCloseLane> {
    let tabs: HashSet<_> = member_tab_ids.iter().map(String::as_str).collect();
    let mut lanes = Vec::new();
    for plan in plans {
        let project_path = if plan.canonical_project_path.trim().is_empty() {
            plan.project_path.clone()
        } else {
            plan.canonical_project_path.clone()
        };
        for task in &plan.tasks {
            let matching_attempts: Vec<_> = task
                .execution_attempts
                .iter()
                .filter(|attempt| tabs.contains(attempt.executor_tab_id.as_str()))
                .collect();
            let assigned_to_closed_tab = task
                .assigned_tab_id
                .as_deref()
                .is_some_and(|tab| tabs.contains(tab))
                || task
                    .lane
                    .tab_id
                    .as_deref()
                    .is_some_and(|tab| tabs.contains(tab));
            let orphaned_in_progress = plan.team_id.as_deref() == Some(team_id)
                && task.status == TaskStatus::InProgress
                && task.assigned_tab_id.is_none()
                && task.lane.tab_id.is_none();
            if matching_attempts.is_empty() && !assigned_to_closed_tab && !orphaned_in_progress {
                continue;
            }
            if assigned_to_closed_tab || orphaned_in_progress {
                push_lane(
                    &mut lanes,
                    &plan.id,
                    &task.id,
                    &project_path,
                    task.lane.worktree_path.as_deref(),
                    task.lane.branch.as_deref(),
                    task.lane.base.as_deref(),
                );
            }
            for attempt in matching_attempts {
                push_lane(
                    &mut lanes,
                    &plan.id,
                    &task.id,
                    &project_path,
                    attempt
                        .worktree_path
                        .as_deref()
                        .or(task.lane.worktree_path.as_deref()),
                    attempt.branch.as_deref().or(task.lane.branch.as_deref()),
                    attempt.base.as_deref().or(task.lane.base.as_deref()),
                );
            }
        }
    }
    lanes
}

pub(super) fn finish_team_lane_cleanup(
    plan_store: &PlanStore,
    lanes: &[TeamCloseLane],
    now_ms: u64,
) -> Result<(), String> {
    let mut outcomes = HashMap::<(String, String), Vec<(&TeamCloseLane, Option<String>)>>::new();
    for lane in lanes {
        let failure = if lane.holds_work {
            Some("lane contains changes or commits beyond its base".to_string())
        } else {
            cleanup_lane(lane).err()
        };
        outcomes
            .entry((lane.plan_id.clone(), lane.task_id.clone()))
            .or_default()
            .push((lane, failure));
    }
    let mut errors = Vec::new();
    for ((plan_id, task_id), lane_outcomes) in outcomes {
        let debt: Vec<_> = lane_outcomes
            .iter()
            .filter_map(|(lane, failure)| failure.as_ref().map(|reason| (*lane, reason)))
            .collect();
        let update = if let Some((primary, _)) = debt.first() {
            TaskLaneUpdate {
                tab_id: Some(None),
                workspace_id: Some(None),
                workspace_name: Some(None),
                worktree_path: Some(primary.worktree_path.clone()),
                branch: Some(primary.branch.clone()),
                base: Some(primary.base.clone()),
                cleanup_status: Some(Some("cleanup_debt".into())),
                cleanup_hints: Some(
                    debt.iter()
                        .map(|(lane, reason)| lane_hint(lane, reason))
                        .collect(),
                ),
                ..Default::default()
            }
        } else {
            TaskLaneUpdate {
                tab_id: Some(None),
                workspace_id: Some(None),
                workspace_name: Some(None),
                worktree_path: Some(None),
                branch: Some(None),
                base: Some(None),
                cleanup_status: Some(Some("done".into())),
                cleanup_hints: Some(Vec::new()),
                ..Default::default()
            }
        };
        if let Err(error) = plan_store.update_task_lane(&plan_id, &task_id, update, now_ms) {
            errors.push(format!("{plan_id}/{task_id}: {error}"));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!("Team closed, but lane state updates failed: {}", errors.join("; ")))
    }
}

fn stage_team_close<F>(
    mux: &MultiplexerState,
    team_id: &str,
    caller: &TeamMutationCaller,
    close_member: &mut F,
) -> Result<StagedTeamClose, String>
where
    F: FnMut(&mut MultiplexerState, &str) -> Result<Vec<String>, String>,
{
    let mut member_tab_ids: Vec<_> = mux
        .snapshot()
        .tabs
        .values()
        .filter(|tab| tab.team_id.as_deref() == Some(team_id))
        .map(|tab| tab.id.clone())
        .collect();
    member_tab_ids.sort();
    super::authorize_team_close(mux, team_id, caller, &member_tab_ids)?;
    let chat_backings = member_tab_ids
        .iter()
        .filter_map(|tab_id| match mux.get_tab(tab_id)?.backing.clone() {
            TabBacking::ChatBackend {
                plugin_id,
                session_id,
            } => Some((plugin_id, session_id)),
            TabBacking::Pty => None,
        })
        .collect();
    let mut transaction = mux.begin_team_close_transaction();
    let mut pty_ids = Vec::new();
    let mut closed_roots = Vec::new();
    for tab_id in &member_tab_ids {
        if transaction.state_mut().get_tab(tab_id).is_some() {
            pty_ids.extend(close_member(transaction.state_mut(), tab_id)?);
            closed_roots.push(tab_id.clone());
        }
    }
    let team = transaction
        .state_mut()
        .remove_team(team_id)
        .ok_or_else(|| format!("Team {team_id} not found"))?;
    Ok(StagedTeamClose {
        team,
        member_tab_ids,
        chat_backings,
        pty_ids,
        closed_roots,
        transaction,
    })
}

fn commit_staged_team_close(
    mux: &mut MultiplexerState,
    staged: StagedTeamClose,
) -> (Team, CloseTeamCleanup) {
    staged.transaction.commit(mux, &staged.closed_roots);
    (
        staged.team,
        CloseTeamCleanup {
            member_tab_ids: staged.member_tab_ids,
            chat_backings: staged.chat_backings,
            pty_ids: staged.pty_ids,
        },
    )
}

fn push_lane(
    lanes: &mut Vec<TeamCloseLane>,
    plan_id: &str,
    task_id: &str,
    project_path: &str,
    worktree_path: Option<&str>,
    branch: Option<&str>,
    base: Option<&str>,
) {
    let worktree_path = non_empty(worktree_path);
    let branch = non_empty(branch);
    if worktree_path.is_none() && branch.is_none() {
        return;
    }
    if lanes.iter().any(|lane| {
        lane.plan_id == plan_id
            && lane.task_id == task_id
            && lane.worktree_path == worktree_path
            && lane.branch == branch
    }) {
        return;
    }
    let mut lane = TeamCloseLane {
        plan_id: plan_id.into(),
        task_id: task_id.into(),
        project_path: project_path.into(),
        worktree_path,
        branch,
        base: non_empty(base),
        holds_work: false,
    };
    lane.holds_work = lane_holds_work(&lane);
    lanes.push(lane);
}

fn lane_holds_work(lane: &TeamCloseLane) -> bool {
    if lane
        .worktree_path
        .as_deref()
        .is_some_and(|path| crate::service::worker_lane::same_path(path, &lane.project_path))
    {
        return true;
    }
    if let Some(path) = lane.worktree_path.as_deref().filter(|path| Path::new(path).exists()) {
        let Ok(status) = git_output(path, &["status", "--porcelain", "--untracked-files=all"])
        else {
            return true;
        };
        if !status.status.success() || !status.stdout.is_empty() {
            return true;
        }
    }
    let reference = lane
        .branch
        .as_deref()
        .or_else(|| lane.worktree_path.as_deref().filter(|path| Path::new(path).exists()).map(|_| "HEAD"));
    let Some(reference) = reference else {
        return false;
    };
    let Some(base) = lane.base.as_deref() else {
        return true;
    };
    let range = format!("{base}..{reference}");
    let Ok(output) = git_output(&lane.project_path, &["rev-list", "--count", &range]) else {
        return true;
    };
    !output.status.success()
        || String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<u64>()
            .map(|count| count > 0)
            .unwrap_or(true)
}

fn cleanup_lane(lane: &TeamCloseLane) -> Result<(), String> {
    if let Some(path) = lane
        .worktree_path
        .as_deref()
        .filter(|path| Path::new(path).exists())
    {
        run_git(&lane.project_path, &["worktree", "remove", path])
            .map_err(|error| format!("worktree {path} could not be removed: {error}"))?;
    }
    if let Some(branch) = lane.branch.as_deref().filter(|branch| branch_exists(lane, branch)) {
        run_git(&lane.project_path, &["branch", "-d", branch])
            .map_err(|error| format!("branch {branch} could not be deleted: {error}"))?;
    }
    Ok(())
}

fn branch_exists(lane: &TeamCloseLane, branch: &str) -> bool {
    let reference = format!("refs/heads/{branch}");
    git_output(
        &lane.project_path,
        &["show-ref", "--verify", "--quiet", &reference],
    )
    .is_ok_and(|output| output.status.success())
}

fn lane_hint(lane: &TeamCloseLane, reason: &str) -> String {
    format!(
        "Team close retained worktree={} branch={} because {reason}",
        lane.worktree_path.as_deref().unwrap_or("<none>"),
        lane.branch.as_deref().unwrap_or("<none>")
    )
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value.map(str::trim).filter(|value| !value.is_empty()).map(str::to_string)
}

fn git_output(cwd: &str, args: &[&str]) -> Result<Output, String> {
    let mut command = Command::new("git");
    process_util::no_window_std(&mut command);
    command
        .current_dir(cwd)
        .args(args)
        .output()
        .map_err(|error| format!("git could not run in '{cwd}': {error}"))
}

fn run_git(cwd: &str, args: &[&str]) -> Result<(), String> {
    let output = git_output(cwd, args)?;
    if output.status.success() {
        return Ok(());
    }
    Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
}
