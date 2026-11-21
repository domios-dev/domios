use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::{Command, Output};

use codeterm_types::team::Team;
use serde::Serialize;

use super::TeamMutationCaller;
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::multiplexer::types::TabBacking;
use codeterm_service_core::plan::LaneOwnership;
use codeterm_service_core::service::plan::lane_ownership::{
    strictest_ownership, unmanaged_lane_reason_listed, LANE_BRANCH_PREFIX,
};
use codeterm_service_core::service::plan::TeamClosePlanRead;
use codeterm_service_core::service::plan::{PlanStore, TaskLaneUpdate};

#[derive(Debug)]
pub struct CloseTeamCleanup {
    pub(super) member_tab_ids: Vec<String>,
    pub(super) agent_providers: Vec<(String, String)>,
    pub(super) chat_backings: Vec<(String, String)>,
    pub(super) pty_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamCloseLane {
    pub(crate) plan_id: String,
    pub(crate) task_id: String,
    pub(crate) project_path: String,
    pub(crate) worktree_path: Option<String>,
    pub(crate) branch: Option<String>,
    pub(crate) base: Option<String>,
    pub(crate) destination: Option<String>,
    pub(crate) hold_reason: Option<String>,
    pub(crate) ownership: Option<LaneOwnership>,
}

pub struct PreparedTeamClose {
    pub(super) member_tab_ids: Vec<String>,
}

pub struct StagedTeamClose {
    team: Team,
    member_tab_ids: Vec<String>,
    agent_providers: Vec<(String, String)>,
    chat_backings: Vec<(String, String)>,
    pty_ids: Vec<String>,
    closed_roots: Vec<String>,
}

pub(super) trait TeamLaneCleanupIo {
    /// Why Domios must not touch this lane at all; asked before any other git.
    fn not_managed(&mut self, lane: &TeamCloseLane) -> Option<String>;
    fn worktree_exists(&mut self, path: &str) -> bool;
    fn branch_exists(&mut self, lane: &TeamCloseLane, branch: &str) -> Result<bool, String>;
    fn hold_reason(&mut self, lane: &TeamCloseLane) -> Option<String>;
    fn remove_lane(&mut self, lane: &TeamCloseLane) -> Result<(), String>;
}

struct GitTeamLaneCleanup;

impl TeamLaneCleanupIo for GitTeamLaneCleanup {
    fn not_managed(&mut self, lane: &TeamCloseLane) -> Option<String> {
        unmanaged_reason(lane, None)
    }

    fn worktree_exists(&mut self, path: &str) -> bool {
        Path::new(path).exists()
    }

    fn branch_exists(&mut self, lane: &TeamCloseLane, branch: &str) -> Result<bool, String> {
        branch_exists(lane, branch)
    }

    fn hold_reason(&mut self, lane: &TeamCloseLane) -> Option<String> {
        lane_hold_reason(lane)
    }

    fn remove_lane(&mut self, lane: &TeamCloseLane) -> Result<(), String> {
        cleanup_lane(lane)
    }
}

#[cfg(any(test, feature = "test-support"))]
pub fn close_team_with_state_change(
    mux: &mut MultiplexerState,
    events: &codeterm_events::EventBroadcaster,
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

#[cfg(any(test, feature = "test-support"))]
pub(super) fn close_team_with_state_change_using<F>(
    mux: &mut MultiplexerState,
    events: &codeterm_events::EventBroadcaster,
    plan_store: &PlanStore,
    team_id: &str,
    caller: &TeamMutationCaller,
    now_ms: u64,
    close_member: F,
) -> Result<(Team, CloseTeamCleanup, Vec<String>), String>
where
    F: FnMut(&mut MultiplexerState, &str) -> Result<Vec<String>, String>,
{
    close_team_with_state_change_using_lane_cleanup(
        mux,
        events,
        plan_store,
        team_id,
        caller,
        now_ms,
        close_member,
        &mut GitTeamLaneCleanup,
    )
}

#[cfg(any(test, feature = "test-support"))]
pub(super) fn close_team_with_state_change_using_lane_cleanup<F>(
    mux: &mut MultiplexerState,
    events: &codeterm_events::EventBroadcaster,
    plan_store: &PlanStore,
    team_id: &str,
    caller: &TeamMutationCaller,
    now_ms: u64,
    mut close_member: F,
    cleanup_io: &mut impl TeamLaneCleanupIo,
) -> Result<(Team, CloseTeamCleanup, Vec<String>), String>
where
    F: FnMut(&mut MultiplexerState, &str) -> Result<Vec<String>, String>,
{
    let initial = stage_team_close(mux, team_id, caller, &mut close_member)?;
    let lanes = team_close_lanes(
        &plan_store
            .team_close_projection(team_id, &initial.member_tab_ids)
            .map_err(|error| error.to_string())?,
        team_id,
        &initial.member_tab_ids,
    );
    let transaction = plan_store
        .close_team_task_attempts_transaction(team_id, &initial.member_tab_ids, now_ms)
        .map_err(|error| error.to_string())?;
    let changed_plan_ids = transaction.changed_plan_ids().to_vec();
    let staged = match stage_team_close(mux, team_id, caller, &mut |mux, tab_id| {
        mux.close_tab_for_team_close(tab_id)
    }) {
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
    let (team, cleanup) = commit_staged_team_close(mux, staged)?;
    codeterm_service_core::service::publish_mux_state_changed(events);
    finish_team_lane_cleanup_using(plan_store, &lanes, now_ms, cleanup_io)?;
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
    let (team, cleanup) = commit_staged_team_close(mux, staged)?;
    Ok((team, cleanup))
}

pub(super) fn team_close_lanes(
    plans: &[TeamClosePlanRead],
    team_id: &str,
    member_tab_ids: &[String],
) -> Vec<TeamCloseLane> {
    team_close_lanes_bounded(plans, team_id, member_tab_ids, usize::MAX)
        .expect("unbounded lane collection cannot overflow")
}

#[derive(Debug)]
pub enum WorkspaceCloseLanePreviewError {
    TooLarge,
    Unavailable(String),
}

pub fn workspace_close_lane_preview(
    plans: &[TeamClosePlanRead],
    team_id: &str,
    member_tab_ids: &[String],
    max_lanes: usize,
) -> Result<
    Vec<codeterm_types::workspace_close_preview::WorkspaceClosePreviewLane>,
    WorkspaceCloseLanePreviewError,
> {
    let lanes = team_close_lanes_bounded(plans, team_id, member_tab_ids, max_lanes)?;
    lanes
        .iter()
        .map(|lane| {
            use codeterm_types::workspace_close_preview::WorkspaceCloseLaneState as State;
            let worktree_exists = match lane.worktree_path.as_deref() {
                Some(path) => match std::fs::metadata(path) {
                    Ok(_) => true,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                    Err(error) => {
                        return Err(WorkspaceCloseLanePreviewError::Unavailable(format!(
                            "could not inspect lane worktree {path}: {error}"
                        )))
                    }
                },
                None => false,
            };
            let branch_exists = match lane.branch.as_deref() {
                Some(branch) => branch_exists(lane, branch)
                    .map_err(WorkspaceCloseLanePreviewError::Unavailable)?,
                None => false,
            };
            let reason = lane_hold_reason(lane);
            if reason.as_deref().is_some_and(lane_fact_unavailable) {
                return Err(WorkspaceCloseLanePreviewError::Unavailable(
                    reason.unwrap_or_else(|| "lane safety facts are unavailable".into()),
                ));
            }
            let orphan = !worktree_exists && branch_exists;
            let state = if orphan {
                State::Orphan
            } else if reason
                .as_deref()
                .is_some_and(|reason| reason.starts_with("dirty worktree:"))
            {
                State::Dirty
            } else if reason
                .as_deref()
                .is_some_and(|reason| reason.contains("unmerged commits"))
            {
                State::Unmerged
            } else if reason.is_some() {
                State::Held
            } else if lane
                .worktree_path
                .as_deref()
                .is_some_and(|_| !worktree_exists)
            {
                State::Orphan
            } else {
                State::Safe
            };
            let detail = reason.or_else(|| {
                orphan.then(|| "the recorded branch has no registered worktree".into())
            });
            Ok(
                codeterm_types::workspace_close_preview::WorkspaceClosePreviewLane {
                    plan_id: lane.plan_id.clone(),
                    task_id: lane.task_id.clone(),
                    path: lane.worktree_path.clone(),
                    branch: lane.branch.clone(),
                    state,
                    detail,
                },
            )
        })
        .collect()
}

fn team_close_lanes_bounded(
    plans: &[TeamClosePlanRead],
    team_id: &str,
    member_tab_ids: &[String],
    max_lanes: usize,
) -> Result<Vec<TeamCloseLane>, WorkspaceCloseLanePreviewError> {
    let tabs: HashSet<_> = member_tab_ids.iter().map(String::as_str).collect();
    let mut lanes = Vec::new();
    for plan in plans {
        let team_owned_plan = plan.team_id.as_deref() == Some(team_id);
        let project_path = if plan.canonical_project_path.trim().is_empty() {
            plan.project_path.clone()
        } else {
            plan.canonical_project_path.clone()
        };
        for task in &plan.tasks {
            if task.has_remote_attempt {
                continue;
            }
            let matching_attempts = &task.attempt_lanes;
            let assigned_to_closed_tab = task
                .assigned_tab_id
                .as_deref()
                .is_some_and(|tab| tabs.contains(tab))
                || task
                    .lane
                    .tab_id
                    .as_deref()
                    .is_some_and(|tab| tabs.contains(tab));
            if matching_attempts.is_empty() && !assigned_to_closed_tab && !team_owned_plan {
                continue;
            }
            if team_owned_plan || assigned_to_closed_tab {
                push_lane(
                    &mut lanes,
                    &plan.id,
                    &task.id,
                    &project_path,
                    task.lane.worktree_path.as_deref(),
                    task.lane.branch.as_deref(),
                    task.lane.base.as_deref(),
                    task.lane.destination.as_deref(),
                    task.lane.ownership,
                );
                if lanes.len() > max_lanes {
                    return Err(WorkspaceCloseLanePreviewError::TooLarge);
                }
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
                    attempt
                        .destination
                        .as_deref()
                        .or(task.lane.destination.as_deref()),
                    attempt.ownership,
                );
                if lanes.len() > max_lanes {
                    return Err(WorkspaceCloseLanePreviewError::TooLarge);
                }
            }
        }
    }
    Ok(lanes)
}

fn lane_fact_unavailable(reason: &str) -> bool {
    if reason.starts_with("dirty worktree:") || reason.contains("unmerged commits") {
        return false;
    }
    reason.starts_with("git ")
        || reason.contains("git could not run")
        || reason.contains("failed in '")
        || reason.starts_with("lane ownership could not be verified")
        || reason.contains("cannot verify merged commits")
}

pub(super) fn finish_team_lane_cleanup(
    plan_store: &PlanStore,
    lanes: &[TeamCloseLane],
    now_ms: u64,
) -> Result<TeamCloseInventory, String> {
    finish_team_lane_cleanup_using(plan_store, lanes, now_ms, &mut GitTeamLaneCleanup)
}

fn finish_team_lane_cleanup_using(
    plan_store: &PlanStore,
    lanes: &[TeamCloseLane],
    now_ms: u64,
    cleanup_io: &mut impl TeamLaneCleanupIo,
) -> Result<TeamCloseInventory, String> {
    let present_lanes: Vec<_> = lanes
        .iter()
        .filter(|lane| {
            lane.worktree_path
                .as_deref()
                .is_some_and(|path| cleanup_io.worktree_exists(path))
                || lane.branch.as_deref().is_some_and(|branch| {
                    matches!(cleanup_io.branch_exists(lane, branch), Ok(true))
                })
        })
        .collect();
    let mut not_managed = Vec::new();
    let mut not_managed_hints = HashMap::<(String, String), Vec<String>>::new();
    let existing_lanes: Vec<_> = present_lanes
        .into_iter()
        .filter(|lane| match cleanup_io.not_managed(lane) {
            Some(reason) => {
                not_managed_hints
                    .entry((lane.plan_id.clone(), lane.task_id.clone()))
                    .or_default()
                    .push(lane_hint(lane, &reason));
                not_managed.push(TeamCloseLaneRecord {
                    path: lane.worktree_path.clone(),
                    branch: lane.branch.clone(),
                    reason: Some(reason),
                });
                false
            }
            None => true,
        })
        .cloned()
        .collect();
    let assessed_lanes: Vec<_> = existing_lanes
        .iter()
        .cloned()
        .map(|mut lane| {
            lane.hold_reason = cleanup_io.hold_reason(&lane);
            lane
        })
        .collect();
    let mut outcomes = HashMap::<(String, String), Vec<(&TeamCloseLane, Option<String>)>>::new();
    let mut inventory = TeamCloseInventory {
        removed: Vec::new(),
        kept: Vec::new(),
        orphan_branches: Vec::new(),
        not_managed,
    };
    for lane in &assessed_lanes {
        if lane
            .worktree_path
            .as_deref()
            .map_or(true, |path| !cleanup_io.worktree_exists(path))
        {
            if let Some(branch) = lane
                .branch
                .as_deref()
                .filter(|branch| matches!(cleanup_io.branch_exists(lane, branch), Ok(true)))
            {
                inventory.orphan_branches.push(branch.to_string());
            }
        }
        let failure = if let Some(reason) = &lane.hold_reason {
            Some(reason.clone())
        } else {
            cleanup_io.remove_lane(lane).err()
        };
        let record = TeamCloseLaneRecord {
            path: lane.worktree_path.clone(),
            branch: lane.branch.clone(),
            reason: failure.clone(),
        };
        if failure.is_some() {
            inventory.kept.push(record);
        } else {
            inventory.removed.push(record);
        }
        outcomes
            .entry((lane.plan_id.clone(), lane.task_id.clone()))
            .or_default()
            .push((lane, failure));
    }
    let mut errors = Vec::new();
    // A foreign checkout leaves nothing for Domios to settle: its task records
    // the untouched path as a hint and its lane references are released.
    for key in not_managed_hints.keys() {
        outcomes.entry(key.clone()).or_default();
    }
    for ((plan_id, task_id), lane_outcomes) in outcomes {
        let untouched = not_managed_hints
            .remove(&(plan_id.clone(), task_id.clone()))
            .unwrap_or_default();
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
                destination: Some(primary.destination.clone()),
                cleanup_status: Some(Some("cleanup_debt".into())),
                cleanup_hints: Some(
                    debt.iter()
                        .map(|(lane, reason)| lane_hint(lane, reason))
                        .chain(untouched)
                        .collect(),
                ),
            }
        } else {
            TaskLaneUpdate {
                tab_id: Some(None),
                workspace_id: Some(None),
                workspace_name: Some(None),
                worktree_path: Some(None),
                branch: Some(None),
                base: Some(None),
                destination: Some(None),
                cleanup_status: Some(Some("done".into())),
                cleanup_hints: Some(untouched),
            }
        };
        if let Err(error) = plan_store.update_task_lane(&plan_id, &task_id, update, now_ms) {
            errors.push(format!("{plan_id}/{task_id}: {error}"));
        }
    }
    for error in errors {
        log::warn!("[team-close] lane state update failed after close: {error}");
    }
    inventory.orphan_branches.sort();
    inventory.orphan_branches.dedup();
    Ok(inventory)
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
    let agent_providers = member_tab_ids
        .iter()
        .filter_map(|tab_id| {
            mux.get_tab(tab_id)
                .and_then(|tab| tab.agent_type.clone())
                .map(|provider_id| (tab_id.clone(), provider_id))
        })
        .collect();
    super::authorize_team_close(mux, team_id, caller, &member_tab_ids)?;
    let chat_backings = member_tab_ids
        .iter()
        .filter_map(|tab_id| match mux.get_tab(tab_id)?.backing.clone() {
            TabBacking::ChatBackend {
                plugin_id,
                session_id,
            } => Some((plugin_id, session_id)),
            TabBacking::Pty | TabBacking::Surface(_) => None,
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
    transaction
        .state_mut()
        .set_team_lifecycle(team_id, codeterm_types::team::TeamLifecycle::Closing)?;
    let team = transaction
        .state_mut()
        .remove_team(team_id)
        .ok_or_else(|| format!("Team {team_id} not found"))?;
    Ok(StagedTeamClose {
        team,
        member_tab_ids,
        agent_providers,
        chat_backings,
        pty_ids,
        closed_roots,
    })
}

fn commit_staged_team_close(
    mux: &mut MultiplexerState,
    staged: StagedTeamClose,
) -> Result<(Team, CloseTeamCleanup), String> {
    mux.commit_team_close(&staged.team.id, &staged.closed_roots)?;
    Ok((
        staged.team,
        CloseTeamCleanup {
            member_tab_ids: staged.member_tab_ids,
            agent_providers: staged.agent_providers,
            chat_backings: staged.chat_backings,
            pty_ids: staged.pty_ids,
        },
    ))
}

pub(crate) fn push_lane(
    lanes: &mut Vec<TeamCloseLane>,
    plan_id: &str,
    task_id: &str,
    project_path: &str,
    worktree_path: Option<&str>,
    branch: Option<&str>,
    base: Option<&str>,
    destination: Option<&str>,
    ownership: Option<LaneOwnership>,
) {
    let worktree_path = non_empty(worktree_path);
    let branch = non_empty(branch);
    if worktree_path.is_none() && branch.is_none() {
        return;
    }
    if let Some(existing) = lanes.iter_mut().find(|lane| {
        lane.plan_id == plan_id
            && lane.task_id == task_id
            && lane.worktree_path == worktree_path
            && lane.branch == branch
            && lane.destination.as_deref() == destination
    }) {
        existing.ownership = strictest_ownership(existing.ownership, ownership);
        return;
    }
    lanes.push(TeamCloseLane {
        plan_id: plan_id.into(),
        task_id: task_id.into(),
        project_path: project_path.into(),
        worktree_path,
        branch,
        base: non_empty(base),
        destination: non_empty(destination),
        hold_reason: None,
        ownership,
    });
}

/// The ownership gate for one inventoried lane; `worktrees` is a preloaded
/// `git worktree list --porcelain` of its project when the caller has one.
pub(crate) fn unmanaged_reason(lane: &TeamCloseLane, worktrees: Option<&str>) -> Option<String> {
    unmanaged_lane_reason_listed(
        lane.ownership,
        &lane.project_path,
        lane.worktree_path.as_deref(),
        lane.branch.as_deref(),
        worktrees,
    )
}

pub(crate) fn lane_hold_reason(lane: &TeamCloseLane) -> Option<String> {
    lane_hold_reason_with_inventory(lane, None, merged_refusal)
}

pub(crate) fn lane_hold_reason_with_inventory(
    lane: &TeamCloseLane,
    inventory: Option<(&str, &HashMap<String, String>)>,
    merged: impl Fn(&str, &str, &str) -> Option<String>,
) -> Option<String> {
    if let Some(reason) = &lane.hold_reason {
        return Some(reason.clone());
    }
    if let Some(reason) = unmanaged_reason(lane, inventory.map(|(worktrees, _)| worktrees)) {
        return Some(reason);
    }
    if let Some(branch) = lane.branch.as_deref() {
        let registrations = if let Some((worktrees, _)) = inventory {
            worktrees.to_string()
        } else {
            match git_output(&lane.project_path, &["worktree", "list", "--porcelain"]) {
                Ok(output) if output.status.success() => {
                    String::from_utf8_lossy(&output.stdout).to_string()
                }
                Ok(output) => {
                    return Some(git_output_error(
                        "worktree list",
                        &lane.project_path,
                        &output,
                    ))
                }
                Err(error) => return Some(error),
            }
        };
        let mut worktree = None;
        for line in registrations.lines() {
            if let Some(path) = line.strip_prefix("worktree ") {
                worktree = Some(path);
            }
            if line.strip_prefix("branch refs/heads/") == Some(branch)
                && worktree
                    .zip(lane.worktree_path.as_deref())
                    .is_none_or(|(actual, expected)| {
                        !codeterm_service_core::service::worker_lane::same_path(actual, expected)
                    })
            {
                return Some("lane branch is checked out in a different worktree".into());
            }
        }
    }
    if lane.worktree_path.as_deref().is_some_and(|path| {
        codeterm_service_core::service::worker_lane::same_path(path, &lane.project_path)
    }) {
        return Some("worktree points to the canonical project checkout".into());
    }
    if let Some(path) = lane
        .worktree_path
        .as_deref()
        .filter(|path| Path::new(path).exists())
    {
        match git_output(path, &["status", "--porcelain", "--untracked-files=all"]) {
            Err(error) => return Some(error),
            Ok(status) if !status.status.success() => {
                return Some(git_output_error("status", path, &status));
            }
            Ok(status) if !status.stdout.is_empty() => {
                return Some(format!(
                    "dirty worktree: git status found uncommitted or untracked changes: {}",
                    String::from_utf8_lossy(&status.stdout).trim()
                ));
            }
            Ok(_) => {}
        }
        if let Some(destination) = lane.destination.as_deref() {
            if let Some(reason) = merged(path, "HEAD", destination) {
                return Some(reason);
            }
        }
        if let Some(branch) = lane.branch.as_deref() {
            match git_output(path, &["symbolic-ref", "--quiet", "--short", "HEAD"]) {
                Ok(output)
                    if output.status.success()
                        && String::from_utf8_lossy(&output.stdout).trim() == branch => {}
                _ => return Some("worktree HEAD does not match its recorded lane branch".into()),
            }
        }
    }
    let branch = lane.branch.as_deref();
    if let Some(branch) = branch {
        if lane.destination.as_deref() == Some(branch) {
            return Some("lane destination is its own branch".into());
        }
        if lane.base.as_deref() == Some(branch) {
            return Some(format!("branch {branch} is the recorded base branch"));
        }
        if !branch.starts_with(LANE_BRANCH_PREFIX) {
            return Some(format!(
                "branch {branch} is outside the {LANE_BRANCH_PREFIX} namespace"
            ));
        }
    }
    let branch = match branch {
        Some(branch) => match inventory.map_or_else(
            || branch_exists(lane, branch),
            |(_, refs)| Ok(refs.contains_key(&format!("refs/heads/{branch}"))),
        ) {
            Ok(true) => Some(branch),
            Ok(false) => None,
            Err(error) => return Some(error),
        },
        None => None,
    };
    let (cwd, reference) = match branch {
        Some(branch) => (lane.project_path.as_str(), branch),
        None => match lane
            .worktree_path
            .as_deref()
            .filter(|path| Path::new(path).exists())
        {
            Some(path) => (path, "HEAD"),
            None => return None,
        },
    };
    let Some(destination) = lane.destination.as_deref() else {
        return Some("lane destination is not recorded; cannot verify merged commits".into());
    };
    let reference = if branch.is_some() {
        format!("refs/heads/{reference}")
    } else {
        reference.to_string()
    };
    merged(cwd, &reference, destination)
}

pub(crate) fn merged_refusal(cwd: &str, reference: &str, destination: &str) -> Option<String> {
    if reference.starts_with('-') || destination.starts_with('-') {
        return Some("invalid lane ancestry reference".into());
    }
    match git_output(
        cwd,
        &["merge-base", "--is-ancestor", reference, destination],
    ) {
        Ok(output) if output.status.success() => None,
        Ok(output) if output.status.code() == Some(1) => Some(format!(
            "branch has unmerged commits ahead of destination {destination}"
        )),
        Ok(output) => Some(git_output_error("merge-base --is-ancestor", cwd, &output)),
        Err(error) => Some(error),
    }
}

#[derive(Debug, Serialize)]
pub struct TeamCloseResult {
    pub remote_members: Vec<codeterm_service_core::service::remote_team::RemoteTeamFinalization>,
    pub team: Team,
    pub inventory: TeamCloseInventory,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub killed_workers: Vec<super::close_busy::TeamCloseBusyWorker>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unfinished_tasks: Vec<super::close_busy::TeamCloseBusyTask>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub flagged_plans: Vec<super::close_busy::TeamCloseFlaggedPlan>,
}

pub use codeterm_wire::api_contracts::TeamCloseInventory;

pub use codeterm_wire::api_contracts::TeamCloseLaneRecord;

fn cleanup_lane(lane: &TeamCloseLane) -> Result<(), String> {
    cleanup_lane_with_tabs(lane, &[])
}

pub(crate) fn cleanup_lane_with_tabs(
    lane: &TeamCloseLane,
    live_tabs: &[codeterm_service_core::service::lane_removal::LiveTabCwd],
) -> Result<(), String> {
    if let Some(reason) = lane_hold_reason(lane) {
        return Err(reason);
    }
    let branch_tip = if let Some(branch) = lane.branch.as_deref() {
        if branch_exists(lane, branch)? {
            let reference = format!("refs/heads/{branch}");
            let output = git_output(&lane.project_path, &["rev-parse", "--verify", &reference])?;
            if !output.status.success() {
                return Err(git_output_error("rev-parse", &lane.project_path, &output));
            }
            let tip = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let destination = lane
                .destination
                .as_deref()
                .ok_or_else(|| "lane destination is not recorded".to_string())?;
            if let Some(reason) = merged_refusal(&lane.project_path, &tip, destination) {
                return Err(reason);
            }
            Some((reference, tip))
        } else {
            None
        }
    } else {
        None
    };
    if let Some(path) = lane.worktree_path.as_deref() {
        codeterm_service_core::service::lane_removal::remove_lane(
            &codeterm_service_core::service::lane_removal::LaneRemovalRequest {
                worktree_path: path.to_string(),
                branch: None,
                canonical_cwd: Some(lane.project_path.clone()),
                force: false,
                ownership: lane.ownership,
            },
            live_tabs,
        )?;
    }
    if let Some((reference, tip)) = branch_tip {
        let current = git_output(&lane.project_path, &["rev-parse", "--verify", &reference])?;
        if !current.status.success() || String::from_utf8_lossy(&current.stdout).trim() != tip {
            return Err(format!(
                "branch {reference} changed during lane removal; branch retained"
            ));
        }
        let branch = reference
            .strip_prefix("refs/heads/")
            .ok_or("invalid lane branch reference")?;
        run_git(&lane.project_path, &["branch", "-d", "--", branch])
            .map_err(|error| format!("branch {reference} could not be deleted: {error}"))?;
    }
    Ok(())
}

fn branch_exists(lane: &TeamCloseLane, branch: &str) -> Result<bool, String> {
    let reference = format!("refs/heads/{branch}");
    let output = git_output(
        &lane.project_path,
        &["show-ref", "--verify", "--quiet", &reference],
    )?;
    if output.status.success() {
        Ok(true)
    } else if output.status.code() == Some(1) {
        Ok(false)
    } else {
        Err(git_output_error("show-ref", &lane.project_path, &output))
    }
}

fn lane_hint(lane: &TeamCloseLane, reason: &str) -> String {
    format!(
        "Team close retained worktree={} branch={} because {reason}",
        lane.worktree_path.as_deref().unwrap_or("<none>"),
        lane.branch.as_deref().unwrap_or("<none>")
    )
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn git_command(cwd: &str, args: &[&str]) -> Command {
    let mut command = codeterm_service_core::process_util::git_command();
    command.current_dir(cwd).args(args);
    command
}

fn git_output(cwd: &str, args: &[&str]) -> Result<Output, String> {
    git_command(cwd, args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .map_err(|error| format!("git could not run in '{cwd}': {error}"))
}

fn git_output_error(operation: &str, cwd: &str, output: &Output) -> String {
    format!(
        "git {operation} failed in '{cwd}' (exit {}): {}",
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stderr).trim()
    )
}

fn run_git(cwd: &str, args: &[&str]) -> Result<(), String> {
    let output = git_command(cwd, args)
        .output()
        .map_err(|error| format!("git could not run in '{cwd}': {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(git_output_error(&args.join(" "), cwd, &output))
}

#[cfg(test)]
mod lane_safety_tests {
    use super::*;

    fn lane(branch: &str, base: &str) -> TeamCloseLane {
        TeamCloseLane {
            plan_id: "plan".into(),
            task_id: "task".into(),
            project_path: String::new(),
            worktree_path: None,
            branch: Some(branch.into()),
            base: Some(base.into()),
            destination: Some(base.into()),
            hold_reason: None,
            ownership: None,
        }
    }

    #[test]
    fn team_close_refuses_to_delete_the_base_branch() {
        assert!(cleanup_lane(&lane("main", "main")).is_err());
    }

    #[test]
    fn team_close_refuses_to_delete_non_lane_branches() {
        assert!(cleanup_lane(&lane("release/retained", "base")).is_err());
    }

    #[test]
    fn workspace_close_lane_preview_classifies_warnings_and_refuses_unavailable_or_over_cap() {
        use codeterm_service_core::plan::{PlanStatus, TaskStatus};
        use codeterm_service_core::service::plan::{TeamCloseLaneRead, TeamCloseTaskRead};
        use codeterm_types::workspace_close_preview::WorkspaceCloseLaneState as State;

        assert!(!lane_fact_unavailable(
            "dirty worktree: git status found uncommitted or untracked changes: ?? a.txt"
        ));
        assert!(!lane_fact_unavailable(
            "branch has unmerged commits ahead of destination main"
        ));
        assert!(!lane_fact_unavailable("foreign checkout, not managed"));
        assert!(!lane_fact_unavailable(
            "lane ownership is not recorded and the lane does not match the Domios-provisioned fingerprint, not managed"
        ));
        assert!(lane_fact_unavailable(
            "git status failed in '/repo' (exit 128): fatal"
        ));
        assert!(lane_fact_unavailable(
            "lane destination is not recorded; cannot verify merged commits"
        ));

        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        let worktrees = temp.path().join("repo-worktrees");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&worktrees).unwrap();
        let repo_str = codeterm_paths::strip_verbatim_prefix(&repo.display().to_string());
        run_git(&repo_str, &["init", "-b", "main"]).unwrap();
        run_git(&repo_str, &["config", "user.email", "test@example.com"]).unwrap();
        run_git(&repo_str, &["config", "user.name", "Test"]).unwrap();
        std::fs::write(repo.join("README.md"), "base\n").unwrap();
        run_git(&repo_str, &["add", "README.md"]).unwrap();
        run_git(&repo_str, &["commit", "-m", "base"]).unwrap();

        let dirty_wt = worktrees.join("dirty");
        let dirty_wt_str = codeterm_paths::strip_verbatim_prefix(&dirty_wt.display().to_string());
        run_git(&repo_str, &["worktree", "add", "-b", "lane/dirty", &dirty_wt_str]).unwrap();
        std::fs::write(dirty_wt.join("untracked.txt"), "dirty\n").unwrap();

        let unmerged_wt = worktrees.join("unmerged");
        let unmerged_wt_str =
            codeterm_paths::strip_verbatim_prefix(&unmerged_wt.display().to_string());
        run_git(&repo_str, &["worktree", "add", "-b", "lane/unmerged", &unmerged_wt_str]).unwrap();
        std::fs::write(unmerged_wt.join("change.txt"), "ahead\n").unwrap();
        run_git(&unmerged_wt_str, &["add", "change.txt"]).unwrap();
        run_git(&unmerged_wt_str, &["commit", "-m", "ahead"]).unwrap();

        run_git(&repo_str, &["branch", "lane/orphan"]).unwrap();

        let mk_task = |id: &str, wt: Option<String>, branch: &str, own: Option<LaneOwnership>| {
            TeamCloseTaskRead {
                id: id.into(),
                name: id.into(),
                status: TaskStatus::InProgress,
                assigned_tab_id: Some("member".into()),
                lane: TeamCloseLaneRead {
                    tab_id: Some("member".into()),
                    worktree_path: wt,
                    branch: Some(branch.into()),
                    base: Some("main".into()),
                    destination: Some("main".into()),
                    ownership: own,
                },
                has_remote_attempt: false,
                member_executor: Some("member".into()),
                attempt_lanes: Vec::new(),
                open_attempts: Vec::new(),
            }
        };
        let plan = TeamClosePlanRead {
            id: "plan-1".into(),
            title: "Plan One".into(),
            status: PlanStatus::Active,
            team_id: Some("team-1".into()),
            project_path: repo_str.clone(),
            canonical_project_path: repo_str.clone(),
            tasks: vec![
                mk_task("t-dirty", Some(dirty_wt_str.clone()), "lane/dirty", Some(LaneOwnership::DomiosLane)),
                mk_task("t-unmerged", Some(unmerged_wt_str.clone()), "lane/unmerged", Some(LaneOwnership::DomiosLane)),
                mk_task("t-orphan", None, "lane/orphan", Some(LaneOwnership::DomiosLane)),
                mk_task("t-foreign", None, "feature/ext", Some(LaneOwnership::ForeignCheckout)),
            ],
        };
        let preview = workspace_close_lane_preview(&[plan], "team-1", &["member".into()], 16).unwrap();
        let states = preview
            .iter()
            .map(|lane| (lane.task_id.as_str(), lane.state))
            .collect::<Vec<_>>();
        assert_eq!(
            states,
            vec![
                ("t-dirty", State::Dirty),
                ("t-unmerged", State::Unmerged),
                ("t-orphan", State::Orphan),
                ("t-foreign", State::Held),
            ]
        );
        assert!(dirty_wt.join("untracked.txt").exists());
        assert!(unmerged_wt.join("change.txt").exists());

        let capped_plan = TeamClosePlanRead {
            id: "plan-cap".into(),
            title: "Cap".into(),
            status: PlanStatus::Active,
            team_id: Some("team-1".into()),
            project_path: repo_str.clone(),
            canonical_project_path: repo_str,
            tasks: vec![
                mk_task("a", None, "lane/orphan", Some(LaneOwnership::DomiosLane)),
                mk_task("b", None, "lane/dirty", Some(LaneOwnership::DomiosLane)),
            ],
        };
        assert!(matches!(
            workspace_close_lane_preview(&[capped_plan], "team-1", &["member".into()], 1),
            Err(WorkspaceCloseLanePreviewError::TooLarge)
        ));

        let broken_plan = TeamClosePlanRead {
            id: "plan-bad".into(),
            title: "Bad".into(),
            status: PlanStatus::Active,
            team_id: Some("team-1".into()),
            project_path: temp.path().join("not-a-repo").display().to_string(),
            canonical_project_path: String::new(),
            tasks: vec![mk_task(
                "t-bad",
                None,
                "lane/orphan",
                Some(LaneOwnership::DomiosLane),
            )],
        };
        assert!(matches!(
            workspace_close_lane_preview(&[broken_plan], "team-1", &["member".into()], 8),
            Err(WorkspaceCloseLanePreviewError::Unavailable(_))
        ));
    }
}

#[cfg(test)]
mod consumer_load_tests {
    use super::*;
    use codeterm_service_core::plan::TaskStatus;
    use codeterm_service_core::service::plan::load_audit::{
        assert_never_loaded, attempt, execution_plan, seed, start, task,
    };

    #[test]
    fn team_close_lanes_never_loads_unmatched_plans() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut member_task = task("task_m", TaskStatus::Done, None);
        let mut lane_attempt = attempt("att_m", "member-tab");
        lane_attempt.branch = Some("lane/member".into());
        member_task.lane.branch = Some("lane/member".into());
        member_task.execution_attempts.push(lane_attempt);
        let member = seed(
            &store,
            execution_plan(dir.path(), "member", "owner", vec![member_task]),
        );
        let mut unrelated = execution_plan(dir.path(), "unrelated", "owner", vec![]);
        unrelated.team_id = Some("team-other".into());
        let unrelated = seed(&store, unrelated);
        start(&store);

        let members = vec!["member-tab".to_string()];
        let lanes = team_close_lanes(
            &store
                .team_close_projection("team-closing", &members)
                .unwrap(),
            "team-closing",
            &members,
        );
        assert!(lanes.iter().all(|lane| lane.plan_id == member));
        assert_never_loaded(&store, &[&member, &unrelated]);
    }
}
