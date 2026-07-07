use std::collections::HashSet;

use crate::plan::coverage::compute_coverage_report;
use crate::plan::PlannerRef;
use crate::service::plan::{build_plan_state_snapshot, PlanError, PlanStateSnapshot};
use crate::service::ServiceContext;
use codeterm_plan::service_plan::state::lane_rows;

impl ServiceContext {
    pub fn build_plan_state(
        &self,
        plan_id: &str,
        now_ms: u64,
    ) -> Result<PlanStateSnapshot, PlanError> {
        let plan = self.plan_store.get(plan_id).ok_or(PlanError::NotFound)?;
        let coverage = compute_coverage_report(&plan);
        let reconcile = self.reconcile_plan(plan_id, false, None, now_ms)?;
        let drift = reconcile.drift;
        let cleanup_debt = reconcile.cleanup_debt;

        let mut live_panes: HashSet<String> = HashSet::new();
        let mut probed: HashSet<String> = HashSet::new();
        for task in &plan.tasks {
            if let Some(pane) = task.assigned_pane_id.as_deref() {
                if probed.insert(pane.to_string()) && self.pane_probe(pane).live_agent {
                    live_panes.insert(pane.to_string());
                }
            }
        }
        if let Some(PlannerRef::Pane { pane_id, .. }) = &plan.planner {
            if probed.insert(pane_id.clone()) && self.pane_probe(pane_id).live_agent {
                live_panes.insert(pane_id.clone());
            }
        }
        let lanes = lane_rows::build_lane_rows(&plan, |p| self.pane_probe(p), now_ms);
        Ok(build_plan_state_snapshot(
            &plan,
            coverage,
            drift,
            cleanup_debt,
            lanes,
            &live_panes,
            now_ms,
        ))
    }
}
