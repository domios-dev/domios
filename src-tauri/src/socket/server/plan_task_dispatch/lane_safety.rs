use serde_json::json;
use std::collections::HashMap;

use crate::plan::{ExecutionIntent, Plan, TaskLaneMetadata};
use crate::service::{classify_mutating_lane, LaneProtection, UnsafeMutatingLane};

use super::super::safety_nudge::{
    SafetyNudge, NUDGE_LANE_BASE_UNRECORDED, NUDGE_LANE_BRANCH_UNRECORDED,
    NUDGE_MERGE_DESTINATION_UNRECORDED, NUDGE_TEAM_IDENTITY_UNRECORDED,
};
use super::super::JsonRpcError;

pub(crate) const REFUSAL_MUTATING_WRITE_IN_CANONICAL: &str =
    "lane.mutating_write_in_canonical_checkout";
pub(crate) const REFUSAL_MUTATING_WRITE_IN_FOREIGN_LANE: &str =
    "lane.mutating_write_in_foreign_lane";

pub(crate) use crate::service::canonical_path_of;

fn refusal(id: &'static str, message: String, safe_alternative: String) -> JsonRpcError {
    JsonRpcError {
        code: -32013,
        message: format!("{message} Safe alternative: {safe_alternative}"),
        data: Some(json!({
            "type": "unsafe_mutating_lane",
            "stage": "lane_safety",
            "violation_id": id,
            "reason": message,
            "safe_alternative": safe_alternative,
        })),
    }
}

/// The ONLY hard stop here, and only where work in progress would be destroyed:
/// a mutating Worker writing in canonical or in a lane another task owns.
pub(crate) fn refuse_unsafe_mutating_write(
    plan: &Plan,
    task_id: &str,
    resolved_worktree: &str,
    intent: ExecutionIntent,
) -> Result<(), JsonRpcError> {
    if intent != ExecutionIntent::Mutating {
        return Ok(());
    }
    match classify_mutating_lane(plan, task_id, resolved_worktree) {
        None => Ok(()),
        Some(UnsafeMutatingLane::Canonical { canonical }) => Err(refusal(
            REFUSAL_MUTATING_WRITE_IN_CANONICAL,
            format!(
                "dispatch refused: a mutating Worker for task '{task_id}' would write in the \
                 canonical checkout '{canonical}'. No Worker was spawned."
            ),
            isolated_lane_update_command(&plan.id, task_id),
        )),
        Some(UnsafeMutatingLane::ForeignLane { worktree, owner }) => Err(refusal(
            REFUSAL_MUTATING_WRITE_IN_FOREIGN_LANE,
            format!(
                "dispatch refused: a mutating Worker for task '{task_id}' would write in \
                 '{worktree}', the lane recorded for task '{owner}'. No Worker was \
                 spawned."
            ),
            isolated_lane_update_command(&plan.id, task_id),
        )),
    }
}

/// Team identity and lane isolation are committed as one value, so no persisted
/// state can carry Team identity without the lane metadata that isolates it.
pub(crate) fn atomic_lane_commitment(lane: &TaskLaneMetadata) -> TaskLaneMetadata {
    let mut committed = lane.clone();
    if committed.worktree_path.is_none() {
        committed.workspace_id = None;
        committed.workspace_name = None;
    }
    committed
}

pub(crate) fn dispatch_safety_nudges(
    plan_id: &str,
    task_id: &str,
    lane: &TaskLaneMetadata,
) -> Vec<SafetyNudge> {
    let update =
        |field: &str| format!("codeterm plan task update {plan_id} {task_id} --{field} <value>");
    let mut nudges = Vec::new();
    if lane.worktree_path.is_some() && lane.branch.is_none() {
        nudges.push(SafetyNudge::new(
            NUDGE_LANE_BRANCH_UNRECORDED,
            "the lane has a worktree but no recorded branch, so integration cannot verify \
             ancestry before merging it."
                .into(),
            update("branch"),
        ));
    }
    if lane.worktree_path.is_some() && lane.base.is_none() {
        nudges.push(SafetyNudge::new(
            NUDGE_LANE_BASE_UNRECORDED,
            "the lane has no recorded base, so a merge cannot prove the lane branched from the \
             integration head."
                .into(),
            update("base"),
        ));
    }
    if lane.worktree_path.is_some() && lane.workspace_id.is_none() {
        nudges.push(SafetyNudge::new(
            NUDGE_TEAM_IDENTITY_UNRECORDED,
            "the lane carries no Team workspace identity, so the Worker's tab will not be \
             Team-visible."
                .into(),
            update("workspace"),
        ));
    }
    if lane.worktree_path.is_some() && lane.destination.is_none() {
        nudges.push(SafetyNudge::new(
            NUDGE_MERGE_DESTINATION_UNRECORDED,
            "the lane has no merge destination, so integration has to guess where the work lands."
                .into(),
            update("destination"),
        ));
    }
    nudges
}

pub(crate) fn reported_executor_tab_statuses(
    plan: &Plan,
    dispatched_task_id: &str,
    is_tab_live: impl Fn(&str) -> bool,
) -> HashMap<(String, String), Option<String>> {
    let mut executor_tabs = HashMap::new();
    for task in plan
        .tasks
        .iter()
        .filter(|task| task.id == dispatched_task_id)
    {
        for attempt in task.execution_attempts.iter().filter(|attempt| {
            attempt.closed_at.is_none()
                && attempt.competition_group_id.is_none()
                && attempt.reported_at.is_some()
        }) {
            let tab_id = &attempt.executor_tab_id;
            executor_tabs.insert(
                (task.id.clone(), attempt.id.clone()),
                is_tab_live(tab_id).then(|| tab_id.clone()),
            );
        }
    }
    executor_tabs
}

/// Occupancy protects work in progress from a second writer, so a declared
/// read-only dispatch is never obstructed by it.
pub(crate) fn preflight_checkout_occupancy(
    plan: &Plan,
    dispatched_task_id: &str,
    checkout_path: &str,
    protection: LaneProtection,
    reported_executor_tab_statuses: &HashMap<(String, String), Option<String>>,
) -> Result<(), JsonRpcError> {
    if protection.is_waived() {
        return Ok(());
    }
    let canonical_path = canonical_path_of(plan);
    let normalized_checkout = codeterm_paths::normalize_cwd(checkout_path);
    let occupant = plan.tasks.iter().find_map(|task| {
        task.execution_attempts.iter().find_map(|attempt| {
            if attempt.closed_at.is_some() {
                return None;
            }
            let executor_tab_status =
                reported_executor_tab_statuses.get(&(task.id.clone(), attempt.id.clone()));
            if matches!(executor_tab_status, Some(tab_id) if tab_id.is_none()) {
                return None;
            }
            let attempt_path = attempt
                .worktree_path
                .as_deref()
                .filter(|path| !path.trim().is_empty())
                .unwrap_or(canonical_path);
            (normalized_checkout == codeterm_paths::normalize_cwd(attempt_path)).then_some((
                task.id.as_str(),
                attempt.id.as_str(),
                executor_tab_status.and_then(|tab_id| tab_id.as_deref()),
            ))
        })
    });
    let Some((occupying_task_id, occupying_attempt_id, executor_tab_id)) = occupant else {
        return Ok(());
    };
    let release_command =
        executor_tab_id.map(|tab_id| format!("codeterm tab close --tab {tab_id}"));
    let update_command = isolated_lane_update_command(&plan.id, dispatched_task_id);
    let dispatch_command = format!(
        "codeterm plan task dispatch {} {}",
        plan.id, dispatched_task_id
    );
    let occupancy = format!(
        "checkout '{checkout_path}' is occupied by live attempt '{occupying_attempt_id}' of task \
         '{occupying_task_id}' in plan '{}'",
        plan.id
    );
    let remediation = format!(
        " No worker was spawned. Assign an existing isolated lane, then dispatch again:\n  \
         {update_command}\n  {dispatch_command}"
    );
    let message = match (executor_tab_id, release_command.as_deref()) {
        (Some(tab_id), Some(command)) => format!(
            "dispatch refused: executor tab '{tab_id}' is still live. Close it with {command} to \
             release the checkout. {occupancy}.{remediation}"
        ),
        _ => format!("dispatch refused: {occupancy}.{remediation}"),
    };
    let mut data = json!({
        "type": "checkout_occupied",
        "stage": "checkout_occupancy",
        "reason": message.clone(),
        "occupying_task_id": occupying_task_id,
        "occupying_attempt_id": occupying_attempt_id,
        "checkout_path": checkout_path,
        "update_command": update_command,
        "dispatch_command": dispatch_command,
    });
    if let (Some(tab_id), Some(command)) = (executor_tab_id, release_command) {
        data["executor_tab_id"] = json!(tab_id);
        data["release_command"] = json!(command);
    }
    Err(JsonRpcError {
        code: -32011,
        message: message.clone(),
        data: Some(data),
    })
}

/// Placeholders, not shell prompts: an agent shell has no tty, so the same
/// literal template has to be runnable once substituted in either shell.
pub(crate) fn isolated_lane_update_command(plan_id: &str, task_id: &str) -> String {
    format!(
        "codeterm plan task update {plan_id} {task_id} --worktree <existing-isolated-worktree-path> \
         --branch <existing-isolated-branch> --base <base-ref>"
    )
}
