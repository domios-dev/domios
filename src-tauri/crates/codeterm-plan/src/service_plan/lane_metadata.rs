use super::TaskLaneUpdate;
use crate::plan::TaskLaneMetadata;

pub(in crate::service_plan) fn normalize_lane_metadata(
    mut lane: TaskLaneMetadata,
) -> TaskLaneMetadata {
    lane.tab_id = non_empty(lane.tab_id);
    lane.workspace_id = non_empty(lane.workspace_id);
    lane.workspace_name = non_empty(lane.workspace_name);
    lane.worktree_path = non_empty(lane.worktree_path);
    lane.branch = non_empty(lane.branch);
    lane.base = non_empty(lane.base);
    lane.destination = non_empty(lane.destination);
    lane.cleanup_status = non_empty(lane.cleanup_status);
    lane.cleanup_hints.retain(|hint| !hint.trim().is_empty());
    lane
}

pub(in crate::service_plan) fn apply_lane_update(
    lane: &mut TaskLaneMetadata,
    update: TaskLaneUpdate,
) {
    if let Some(value) = update.tab_id {
        lane.tab_id = value;
    }
    if let Some(value) = update.workspace_id {
        lane.workspace_id = value;
    }
    if let Some(value) = update.workspace_name {
        lane.workspace_name = value;
    }
    if let Some(value) = update.worktree_path {
        lane.worktree_path = value;
    }
    if let Some(value) = update.branch {
        lane.branch = value;
    }
    if let Some(value) = update.base {
        lane.base = value;
    }
    if let Some(value) = update.destination {
        lane.destination = value;
    }
    if let Some(value) = update.cleanup_status {
        lane.cleanup_status = value;
    }
    if let Some(value) = update.cleanup_hints {
        lane.cleanup_hints = value;
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.and_then(|s| {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}
