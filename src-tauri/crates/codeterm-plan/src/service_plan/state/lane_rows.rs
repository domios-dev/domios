use serde::Serialize;

use crate::plan::lane_liveness::{derive_lane_liveness, LaneInputs, LaneLiveness, LaneThresholds};
use crate::plan::reconcile::TabProbe;
use crate::plan::{Plan, Task, TaskStatus};

/// One row of the `plan lanes` hygiene table: a lane task joined with its
/// TaskLaneMetadata and a liveness verdict keyed off the tab's OWN session.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LaneRow {
    pub task_id: String,
    pub handle: Option<String>,
    pub title: String,
    pub status: TaskStatus,
    pub tab_id: Option<String>,
    pub liveness: LaneLiveness,
    pub branch: Option<String>,
    pub worktree: Option<String>,
    pub cleanup_status: Option<String>,
    pub uncleaned: bool,
}

fn is_open(status: &TaskStatus) -> bool {
    matches!(
        status,
        TaskStatus::Pending | TaskStatus::InProgress | TaskStatus::InReview
    )
}

// A lane is "uncleaned" once its task is terminal but the worktree/branch was
// never reclaimed. cleanup_status None on a terminal lane with a recorded
// worktree/branch counts as debt.
fn lane_uncleaned(task: &Task) -> bool {
    let has_artifacts = task.lane.worktree_path.is_some() || task.lane.branch.is_some();
    match task.lane.cleanup_status.as_deref() {
        Some("cleaned") | Some("done") | Some("released") => false,
        Some(_) => has_artifacts,
        None => task.status.is_terminal() && has_artifacts,
    }
}

fn lane_liveness<F>(task: &Task, tab_probe: &mut F, now_ms: u64) -> (Option<String>, LaneLiveness)
where
    F: FnMut(&str) -> TabProbe,
{
    let tab_id = task
        .lane
        .tab_id
        .clone()
        .or_else(|| task.assigned_tab_id.clone());
    let has_peer = task.peer.is_some();
    // Local-only v1: a mesh/peer lane is treated as offline (no mesh probe),
    // never guessed gone — a sleeping peer must not read as dead.
    let probe = tab_id.as_deref().map(|p| tab_probe(p));
    let inputs = LaneInputs {
        has_assigned_tab: tab_id.is_some(),
        tab_present_local: probe.as_ref().map(|p| p.exists).unwrap_or(false),
        has_peer,
        peer_online: if has_peer { Some(false) } else { None },
        tracker_active: probe.as_ref().map(|p| p.live_agent),
        last_activity_ms: task.status_changed_at,
        now_ms,
    };
    let liveness = derive_lane_liveness(&inputs, &LaneThresholds::default());
    (tab_id, liveness)
}

/// Build one row per open lane (pending/in_progress/in_review). Liveness is
/// derived from the tab's own probe — unknown tabs stay honestly NoTab.
pub fn build_lane_rows<F>(plan: &Plan, mut tab_probe: F, now_ms: u64) -> Vec<LaneRow>
where
    F: FnMut(&str) -> TabProbe,
{
    plan.tasks
        .iter()
        .filter(|t| is_open(&t.status))
        .map(|t| {
            let (tab_id, liveness) = lane_liveness(t, &mut tab_probe, now_ms);
            LaneRow {
                task_id: t.id.clone(),
                handle: t.handle.clone(),
                title: t.name.clone(),
                status: t.status.clone(),
                tab_id,
                liveness,
                branch: t.lane.branch.clone(),
                worktree: t.lane.worktree_path.clone(),
                cleanup_status: t.lane.cleanup_status.clone(),
                uncleaned: lane_uncleaned(t),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
