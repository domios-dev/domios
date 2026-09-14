use std::collections::HashSet;

use crate::plan::coverage::compute_coverage_report;
use crate::plan::peer::resolve_planner_ref;
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

        let mut live_tabs: HashSet<String> = HashSet::new();
        let mut probed: HashSet<String> = HashSet::new();
        for task in &plan.tasks {
            if let Some(tab) = task.assigned_tab_id.as_deref() {
                if probed.insert(tab.to_string()) && self.tab_probe(tab).live_agent {
                    live_tabs.insert(tab.to_string());
                }
            }
        }
        if let Some(PlannerRef::Tab { tab_id, .. }) = &plan.planner {
            if probed.insert(tab_id.clone()) && self.tab_probe(tab_id).live_agent {
                live_tabs.insert(tab_id.clone());
            }
        }
        let lanes = lane_rows::build_lane_rows(&plan, |p| self.tab_probe(p), now_ms);
        let planner_resolution =
            resolve_planner_ref(&plan.planner, &plan.owner, |tab_id| self.tab_probe(tab_id));
        let mut snapshot = build_plan_state_snapshot(
            &plan,
            coverage,
            drift,
            cleanup_debt,
            lanes,
            &live_tabs,
            Some(planner_resolution),
            now_ms,
        );
        snapshot.mission_control.kpi = super::mission_control_ext::plan_kpi(self, plan_id);
        snapshot.mission_control.spend = super::mission_control_ext::machine_spend(self);
        Ok(snapshot)
    }
}
