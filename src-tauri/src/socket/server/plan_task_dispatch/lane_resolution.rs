use serde_json::json;

use crate::plan::{ExecutionIntent, Plan, TaskLaneMetadata};
use crate::service::{
    classify_mutating_lane, recorded_lane_matches, verify_worker_lane, LaneProtection,
    UnsafeMutatingLane, WorkerLaneProvenance,
};

use super::super::JsonRpcError;
use super::lane_safety::refuse_unsafe_mutating_write;

pub(super) fn invalid_dispatch(reason: impl Into<String>) -> JsonRpcError {
    let reason = reason.into();
    JsonRpcError {
        code: -32602,
        message: format!("invalid task dispatch: {reason}"),
        data: Some(json!({
            "type": "invalid_task_dispatch",
            "stage": "cwd_validation",
            "reason": reason,
        })),
    }
}

pub(super) fn checkout_required(reason: impl Into<String>) -> JsonRpcError {
    let reason = reason.into();
    JsonRpcError {
        code: -32012,
        message: format!("repository checkout required: {reason}"),
        data: Some(json!({
            "type": "checkout_required",
            "stage": "checkout_preflight",
            "reason": reason,
        })),
    }
}

#[derive(Clone, Debug)]
pub(super) struct DispatchLaneResolution {
    pub(super) spawn_cwd: String,
    pub(super) existing: Option<WorkerLaneProvenance>,
}

pub(super) fn resolve_dispatch_lane(
    explicit_cwd: Option<&str>,
    workspace_cwd: Option<&str>,
    caller_cwd: Option<&str>,
    task_lane: &TaskLaneMetadata,
    protection: LaneProtection,
) -> Result<DispatchLaneResolution, JsonRpcError> {
    let candidates = [
        (
            "recorded worktree",
            task_lane.worktree_path.as_deref(),
            task_lane.branch.as_deref(),
            task_lane.base.as_deref(),
        ),
        ("explicit cwd", explicit_cwd, None, None),
        ("target workspace cwd", workspace_cwd, None, None),
        ("caller cwd", caller_cwd, None, None),
    ];
    let mut attempted = Vec::new();

    for (label, path, branch, base) in candidates {
        let Some(path) = path.filter(|path| !path.trim().is_empty()) else {
            continue;
        };
        attempted.push(format!("{label} '{path}'"));
        if let Ok(existing) = verify_worker_lane(path, path, branch, base, protection) {
            return Ok(DispatchLaneResolution {
                spawn_cwd: existing.worktree_path.clone(),
                existing: Some(existing),
            });
        }
    }

    Err(checkout_required(format!(
        "no repository checkout resolved; tried in order: {}",
        attempted.join(", ")
    )))
}

/// Where a mutating dispatch will actually land, decided BEFORE any tab exists.
///
/// Resolution above answers "which checkout"; this answers "is that checkout a
/// lane this task may write in". The two were one question, which is how an
/// unrecorded dispatch reached canonical and was refused there instead of being
/// given the isolated lane it was always supposed to get (codeterm#450).
#[derive(Clone, Debug)]
pub(super) enum DispatchLanePlacement {
    /// The resolved checkout is this task's own isolated lane.
    Existing(WorkerLaneProvenance),
    /// No lane exists yet; branch a sibling lane off this checkout.
    Provision { base_checkout: String },
}

impl DispatchLanePlacement {
    pub(super) fn spawn_cwd(&self) -> &str {
        match self {
            Self::Existing(provenance) => provenance.worktree_path.as_str(),
            Self::Provision { base_checkout } => base_checkout.as_str(),
        }
    }

    pub(super) fn provisions(&self) -> bool {
        matches!(self, Self::Provision { .. })
    }
}

pub(super) fn plan_dispatch_lane_placement(
    plan: &Plan,
    task_id: &str,
    task_lane: &TaskLaneMetadata,
    resolved: WorkerLaneProvenance,
) -> Result<DispatchLanePlacement, JsonRpcError> {
    let recorded = task_lane
        .worktree_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty());
    if let Some(recorded) = recorded {
        // A recorded lane that did not verify must never be replaced silently:
        // the work in it is exactly what the record exists to protect.
        if !recorded_lane_matches(Some(recorded), &resolved.worktree_path) {
            return Err(checkout_required(format!(
                "task '{task_id}' records the isolated lane '{recorded}', which did not verify; \
                 dispatch resolved '{}' instead. No Worker was spawned.",
                resolved.worktree_path
            )));
        }
        refuse_unsafe_mutating_write(
            plan,
            task_id,
            &resolved.worktree_path,
            ExecutionIntent::Mutating,
        )?;
        return Ok(DispatchLanePlacement::Existing(resolved));
    }
    // Canonical is the integration checkout, never the worker checkout: it is
    // the base to branch from, so provisioning replaces the refusal.
    if matches!(
        classify_mutating_lane(plan, task_id, &resolved.worktree_path),
        Some(UnsafeMutatingLane::Canonical { .. })
    ) {
        return Ok(DispatchLanePlacement::Provision {
            base_checkout: resolved.worktree_path,
        });
    }
    refuse_unsafe_mutating_write(
        plan,
        task_id,
        &resolved.worktree_path,
        ExecutionIntent::Mutating,
    )?;
    Ok(DispatchLanePlacement::Existing(resolved))
}
