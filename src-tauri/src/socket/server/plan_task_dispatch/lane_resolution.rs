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
    canonical_cwd: &str,
    task_lane: &TaskLaneMetadata,
    protection: LaneProtection,
) -> Result<DispatchLaneResolution, JsonRpcError> {
    if let Some(explicit) = explicit_cwd {
        if task_lane.worktree_path.as_deref().is_some_and(|recorded| {
            !crate::service::ServiceContext::paths_equal(explicit, recorded)
        }) {
            return Err(invalid_dispatch(format!(
                "explicit cwd '{}' conflicts with recorded lane worktree '{}'",
                explicit,
                task_lane.worktree_path.as_deref().unwrap_or_default()
            )));
        }
        let existing = verify_worker_lane(
            canonical_cwd,
            explicit,
            task_lane.branch.as_deref(),
            task_lane.base.as_deref(),
            protection,
        )
        .map_err(invalid_dispatch)?;
        return Ok(DispatchLaneResolution {
            spawn_cwd: existing.worktree_path.clone(),
            existing: Some(existing),
        });
    }
    if let Some(recorded) = task_lane.worktree_path.as_deref() {
        let existing = verify_worker_lane(
            canonical_cwd,
            recorded,
            task_lane.branch.as_deref(),
            task_lane.base.as_deref(),
            protection,
        )
        .map_err(invalid_dispatch)?;
        return Ok(DispatchLaneResolution {
            spawn_cwd: existing.worktree_path.clone(),
            existing: Some(existing),
        });
    }
    Ok(DispatchLaneResolution {
        spawn_cwd: canonical_cwd.to_string(),
        existing: None,
    })
}
