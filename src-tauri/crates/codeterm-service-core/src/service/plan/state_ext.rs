use std::collections::HashSet;

use crate::plan::coverage::compute_coverage_report;
use crate::plan::peer::resolve_planner_ref;
use crate::plan::PlannerRef;
use crate::service::plan::{build_plan_state_snapshot, PlanError, PlanStateSnapshot};
use crate::service::ServiceContext;
use codeterm_plan::service_plan::state::{lane_rows, window::StateWindowRequest};

impl ServiceContext {
    pub fn build_plan_board_state(
        &self,
        plan_id: &str,
        now_ms: u64,
        event_offset: usize,
        include_events: bool,
    ) -> Result<serde_json::Value, PlanError> {
        let mut read = self.plan_store.board_state_projection(
            plan_id,
            now_ms,
            event_offset,
            include_events,
        )?;
        let suspended = self.curator_suspended_reason_for_fields(
            &read.mode,
            &read.status,
            &read.team_id,
            read.complete,
        );
        read.value["control_loop"]["suspended_reason"] = serde_json::json!(suspended);
        if suspended.is_some() {
            read.value["control_loop"]["next_at"] = serde_json::Value::Null;
        }
        read.value["mission_control"]["spend"] =
            serde_json::json!(super::mission_control_ext::machine_spend(self));
        Ok(read.value)
    }

    pub fn build_plan_state(
        &self,
        plan_id: &str,
        now_ms: u64,
    ) -> Result<PlanStateSnapshot, PlanError> {
        self.build_plan_state_with_window(plan_id, now_ms, StateWindowRequest::default())
    }

    pub fn build_plan_state_with_window(
        &self,
        plan_id: &str,
        now_ms: u64,
        window: StateWindowRequest,
    ) -> Result<PlanStateSnapshot, PlanError> {
        let read = self.plan_store.status_projection(plan_id, now_ms)?;
        let plan = read.input();
        let coverage = compute_coverage_report(plan);
        let reconcile = self.reconcile_plan_record(plan, false, None, now_ms)?;
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
        let lanes = lane_rows::build_lane_rows(plan, |p| self.tab_probe(p), now_ms);
        let planner_resolution =
            resolve_planner_ref(&plan.planner, &plan.owner, |tab_id| self.tab_probe(tab_id));
        let mut snapshot = build_plan_state_snapshot(
            plan,
            coverage,
            drift,
            cleanup_debt,
            lanes,
            &live_tabs,
            Some(planner_resolution),
            now_ms,
        );
        read.apply_history_facts(&mut snapshot, now_ms);
        snapshot.mission_control.kpi = super::mission_control_ext::plan_kpi(self, plan_id);
        snapshot.control_loop.suspended_reason = self.curator_suspended_reason_for_fields(
            &plan.mode,
            &plan.status,
            &plan.team_id,
            snapshot.task_counts.total > 0
                && snapshot.task_counts.done + snapshot.task_counts.cancelled
                    == snapshot.task_counts.total,
        );
        if snapshot.control_loop.suspended_reason.is_some() {
            snapshot.control_loop.next_at = None;
        }
        snapshot.mission_control.spend = super::mission_control_ext::machine_spend(self);
        snapshot.window_details(window);
        Ok(snapshot)
    }
}
