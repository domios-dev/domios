use serde_json::Value;

use crate::plan::TaskLaneMetadata;
use crate::service::plan::TaskLaneUpdate;

use super::JsonRpcError;

pub(super) fn lane_metadata_field(p: Option<&Value>) -> Result<TaskLaneMetadata, JsonRpcError> {
    p.and_then(|x| x.get("lane"))
        .map(|v| {
            serde_json::from_value(v.clone()).map_err(|e| JsonRpcError {
                code: -32602,
                message: format!("invalid lane metadata: {e}"),
                data: None,
            })
        })
        .transpose()
        .map(|lane| lane.unwrap_or_default())
}

pub(super) fn lane_update_field(p: Option<&Value>) -> Result<Option<TaskLaneUpdate>, JsonRpcError> {
    let Some(lane) = p.and_then(|x| x.get("lane")) else {
        return Ok(None);
    };
    let mut update = TaskLaneUpdate {
        tab_id: lane.get("tab_id").map(|v| v.as_str().map(str::to_string)),
        workspace_id: lane
            .get("workspace_id")
            .map(|v| v.as_str().map(str::to_string)),
        workspace_name: lane
            .get("workspace_name")
            .map(|v| v.as_str().map(str::to_string)),
        worktree_path: lane
            .get("worktree_path")
            .map(|v| v.as_str().map(str::to_string)),
        branch: lane.get("branch").map(|v| v.as_str().map(str::to_string)),
        base: lane.get("base").map(|v| v.as_str().map(str::to_string)),
        destination: lane
            .get("destination")
            .map(|v| v.as_str().map(str::to_string)),
        cleanup_status: lane
            .get("cleanup_status")
            .map(|v| v.as_str().map(str::to_string)),
        ..Default::default()
    };
    if lane.get("cleanup_hints").is_some() {
        update.cleanup_hints = Some(
            serde_json::from_value(lane.get("cleanup_hints").cloned().unwrap_or(Value::Null))
                .map_err(|e| JsonRpcError {
                    code: -32602,
                    message: format!("invalid cleanup_hints: {e}"),
                    data: None,
                })?,
        );
    }
    Ok((!update.is_empty()).then_some(update))
}
