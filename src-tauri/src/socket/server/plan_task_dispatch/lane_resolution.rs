use serde_json::json;

use crate::plan::TaskLaneMetadata;
use crate::service::{verify_worker_lane, LaneProtection, WorkerLaneProvenance};

use super::super::JsonRpcError;

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
        match verify_worker_lane(path, path, branch, base, protection) {
            Ok(existing) => {
                return Ok(DispatchLaneResolution {
                    spawn_cwd: existing.worktree_path.clone(),
                    existing: Some(existing),
                })
            }
            // Carry each candidate's own reason: one generic message here once
            // hid an unrelated git incompatibility behind "no checkout resolved".
            Err(reason) => attempted.push(format!("{label} '{path}': {reason}")),
        }
    }

    Err(checkout_required(format!(
        "no repository checkout resolved; tried in order: {}",
        attempted.join(", ")
    )))
}
