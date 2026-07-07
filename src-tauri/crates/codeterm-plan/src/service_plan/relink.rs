//! Hierarchy relink on plan transfer: repoint the parent edge of a plan's
//! planner and its live (in_progress) workers to the new owner pane so reports
//! and nudges route correctly post-transfer.

use super::ports::PlanLaneStatePort;
use crate::plan::{Plan, PlannerRef, TaskStatus};

/// Pane ids to relink for a plan: in_progress-lane workers + planner pane.
/// Enumerated from the plan itself — never an all-pane scan.
fn relink_targets(plan: &Plan) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |pid: &str| {
        let pid = pid.trim();
        if !pid.is_empty() && !out.iter().any(|p| p == pid) {
            out.push(pid.to_string());
        }
    };
    for task in &plan.tasks {
        if task.status == TaskStatus::InProgress {
            if let Some(pid) = task.assigned_pane_id.as_deref() {
                push(pid);
            }
        }
    }
    if let Some(PlannerRef::Pane { pane_id, .. }) = &plan.planner {
        push(pane_id);
    }
    out
}

/// Repoint planner + in_progress workers to `new_owner_pane`. Returns the number
/// of panes whose parent edge was moved. Absent panes and a target equal to the
/// new owner are graceful no-ops; idempotent.
pub fn relink_to_owner(mux: &mut dyn PlanLaneStatePort, plan: &Plan, new_owner_pane: &str) -> usize {
    let new_owner_pane = new_owner_pane.trim();
    if new_owner_pane.is_empty() {
        return 0;
    }
    let mut relinked = 0;
    for pane_id in relink_targets(plan) {
        if pane_id != new_owner_pane && mux.repoint_parent(&pane_id, new_owner_pane) {
            relinked += 1;
        }
    }
    relinked
}
