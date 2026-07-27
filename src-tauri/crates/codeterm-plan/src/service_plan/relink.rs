//! Hierarchy relink on plan transfer: repoint the parent edge of a plan's
//! planner and its live (in_progress) workers to the new owner tab so reports
//! and nudges route correctly post-transfer.

use super::ports::PlanLaneStatePort;
use crate::plan::{Plan, PlannerRef, TaskStatus};

/// Tab ids to relink for a plan: in_progress-lane workers + planner tab.
/// Enumerated from the plan itself — never an all-tab scan.
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
            if let Some(pid) = task.assigned_tab_id.as_deref() {
                push(pid);
            }
        }
    }
    if let Some(PlannerRef::Tab { tab_id, .. }) = &plan.planner {
        push(tab_id);
    }
    out
}

/// Repoint planner + in_progress workers to `new_owner_tab`. Returns the number
/// of tabs whose parent edge was moved. Absent tabs and a target equal to the
/// new owner are graceful no-ops; idempotent.
pub fn relink_to_owner(
    mux: &mut dyn PlanLaneStatePort,
    plan: &Plan,
    new_owner_tab: &str,
) -> usize {
    let new_owner_tab = new_owner_tab.trim();
    if new_owner_tab.is_empty() {
        return 0;
    }
    let mut relinked = 0;
    for tab_id in relink_targets(plan) {
        if tab_id != new_owner_tab && mux.repoint_parent(&tab_id, new_owner_tab) {
            relinked += 1;
        }
    }
    relinked
}
