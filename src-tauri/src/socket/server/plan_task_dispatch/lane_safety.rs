use serde_json::json;

use crate::plan::{ExecutionIntent, Plan, TaskLaneMetadata};

use super::super::safety_nudge::{
    SafetyNudge, NUDGE_LANE_BASE_UNRECORDED, NUDGE_LANE_BRANCH_UNRECORDED,
    NUDGE_MERGE_DESTINATION_UNRECORDED, NUDGE_TEAM_IDENTITY_UNRECORDED,
};
use super::super::JsonRpcError;

pub(crate) const REFUSAL_MUTATING_WRITE_IN_CANONICAL: &str =
    "lane.mutating_write_in_canonical_checkout";
pub(crate) const REFUSAL_MUTATING_WRITE_IN_FOREIGN_LANE: &str =
    "lane.mutating_write_in_foreign_lane";

pub(crate) fn canonical_path_of(plan: &Plan) -> &str {
    if plan.canonical_project_path.trim().is_empty() {
        plan.project_path.as_str()
    } else {
        plan.canonical_project_path.as_str()
    }
}

fn same_checkout(a: &str, b: &str) -> bool {
    codeterm_paths::normalize_cwd(a) == codeterm_paths::normalize_cwd(b)
}

fn foreign_lane_owner<'a>(plan: &'a Plan, task_id: &str, worktree: &str) -> Option<&'a str> {
    plan.tasks
        .iter()
        .filter(|task| task.id != task_id && task.handle.as_deref() != Some(task_id))
        .find(|task| {
            task.lane
                .worktree_path
                .as_deref()
                .is_some_and(|recorded| same_checkout(recorded, worktree))
        })
        .map(|task| task.id.as_str())
}

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
    let canonical = canonical_path_of(plan);
    if same_checkout(resolved_worktree, canonical) {
        return Err(refusal(
            REFUSAL_MUTATING_WRITE_IN_CANONICAL,
            format!(
                "dispatch refused: a mutating Worker for task '{task_id}' would write in the \
                 canonical checkout '{canonical}'. No Worker was spawned."
            ),
            isolated_lane_update_command(&plan.id, task_id),
        ));
    }
    if let Some(owner) = foreign_lane_owner(plan, task_id, resolved_worktree) {
        return Err(refusal(
            REFUSAL_MUTATING_WRITE_IN_FOREIGN_LANE,
            format!(
                "dispatch refused: a mutating Worker for task '{task_id}' would write in \
                 '{resolved_worktree}', the lane recorded for task '{owner}'. No Worker was \
                 spawned."
            ),
            isolated_lane_update_command(&plan.id, task_id),
        ));
    }
    Ok(())
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
    let update = |field: &str| {
        format!("codeterm plan task update {plan_id} {task_id} --{field} <value>")
    };
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

pub(crate) fn preflight_checkout_occupancy(
    plan: &Plan,
    dispatched_task_id: &str,
    checkout_path: &str,
) -> Result<(), JsonRpcError> {
    let canonical_path = canonical_path_of(plan);
    let normalized_checkout = codeterm_paths::normalize_cwd(checkout_path);
    let occupant = plan.tasks.iter().find_map(|task| {
        task.execution_attempts.iter().find_map(|attempt| {
            if attempt.closed_at.is_some() {
                return None;
            }
            let attempt_path = attempt
                .worktree_path
                .as_deref()
                .filter(|path| !path.trim().is_empty())
                .unwrap_or(canonical_path);
            (normalized_checkout == codeterm_paths::normalize_cwd(attempt_path))
                .then_some((task.id.as_str(), attempt.id.as_str()))
        })
    });
    let Some((occupying_task_id, occupying_attempt_id)) = occupant else {
        return Ok(());
    };
    let update_command = isolated_lane_update_command(&plan.id, dispatched_task_id);
    let dispatch_command = format!(
        "codeterm plan task dispatch {} {}",
        plan.id, dispatched_task_id
    );
    let message = format!(
        "dispatch refused: checkout '{checkout_path}' is occupied by live attempt \
         '{occupying_attempt_id}' of task '{occupying_task_id}' in plan '{}'. No worker was \
         spawned. Assign an existing isolated lane, then dispatch again:\n  {update_command}\n  \
         {dispatch_command}",
        plan.id
    );
    Err(JsonRpcError {
        code: -32011,
        message: message.clone(),
        data: Some(json!({
            "type": "checkout_occupied",
            "stage": "checkout_occupancy",
            "reason": message,
            "occupying_task_id": occupying_task_id,
            "occupying_attempt_id": occupying_attempt_id,
            "checkout_path": checkout_path,
            "update_command": update_command,
            "dispatch_command": dispatch_command,
        })),
    })
}

#[cfg(windows)]
pub(crate) fn isolated_lane_update_command(plan_id: &str, task_id: &str) -> String {
    format!(
        "codeterm plan task update {plan_id} {task_id} --worktree (Read-Host 'Existing isolated worktree path') --branch (Read-Host 'Existing isolated branch') --base (Read-Host 'Base ref')"
    )
}

#[cfg(not(windows))]
pub(crate) fn isolated_lane_update_command(plan_id: &str, task_id: &str) -> String {
    format!(
        "codeterm plan task update {plan_id} {task_id} \
         --worktree \"$(printf 'Existing isolated worktree path: ' >/dev/tty; IFS= read -r value </dev/tty; printf '%s' \"$value\")\" \
         --branch \"$(printf 'Existing isolated branch: ' >/dev/tty; IFS= read -r value </dev/tty; printf '%s' \"$value\")\" \
         --base \"$(printf 'Base ref: ' >/dev/tty; IFS= read -r value </dev/tty; printf '%s' \"$value\")\""
    )
}
