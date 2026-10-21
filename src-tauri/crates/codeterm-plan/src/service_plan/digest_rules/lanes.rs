use std::collections::BTreeMap;

use crate::plan::Plan;

pub(super) fn recorded_finished_lanes(plan: &Plan) -> (usize, Vec<String>) {
    let mut lanes = BTreeMap::new();
    for task in plan.tasks.iter().filter(|task| task.status.is_terminal()) {
        let status = task.lane.cleanup_status.as_deref().unwrap_or("pending");
        if ["done", "cleaned", "released", "complete"]
            .iter()
            .any(|value| status.eq_ignore_ascii_case(value))
        {
            continue;
        }
        let hints = task.lane.cleanup_hints.join("; ").to_ascii_lowercase();
        let unsafe_lane = hints.contains("dirty") || hints.contains("unmerged");
        let safety = if unsafe_lane {
            "retain dirty/unmerged"
        } else {
            status
        };
        let mut record = |peer: Option<&str>, path: Option<&str>| {
            let Some(path) = path.map(str::trim).filter(|path| !path.is_empty()) else {
                return;
            };
            let label = match peer {
                Some(peer) => format!("{peer}:{path}"),
                None => path.to_string(),
            };
            lanes
                .entry((peer.map(str::to_string), path.to_string()))
                .or_insert_with(|| (unsafe_lane, format!("{label} ({safety})")));
        };
        record(task.peer.as_deref(), task.lane.worktree_path.as_deref());
        for attempt in &task.execution_attempts {
            record(
                attempt.mesh_peer.as_deref(),
                attempt
                    .remote_worktree_path
                    .as_deref()
                    .or(attempt.worktree_path.as_deref()),
            );
        }
    }
    let count = lanes.len();
    let mut candidates: Vec<_> = lanes.into_values().collect();
    candidates.sort_by_key(|(unsafe_lane, _)| !unsafe_lane);
    candidates.truncate(5);
    (
        count,
        candidates.into_iter().map(|(_, label)| label).collect(),
    )
}

#[cfg(test)]
mod tests;
