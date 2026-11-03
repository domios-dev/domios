use super::state::mission_control::{BounceSummary, EventLog};
use super::state::PlanStateSnapshot;
use super::{PlanError, PlanStore};
use crate::plan::Plan;
use std::collections::HashMap;

pub struct CuratorScheduleRead {
    plan: Plan,
}

impl CuratorScheduleRead {
    pub fn plan(&self) -> &Plan {
        &self.plan
    }
}

pub struct PlanStatusRead {
    pub(super) plan: Plan,
    pub(super) event_log: EventLog,
    pub(super) bounces: BounceSummary,
    pub(super) held_reviews: HashMap<String, (usize, Option<u64>)>,
    pub(super) lane_history_count: usize,
    pub(super) undelivered_requests: usize,
    pub(super) delivery_uncertain: bool,
}

impl PlanStatusRead {
    pub fn plan(&self) -> &Plan {
        &self.plan
    }

    pub fn apply_history_facts(&self, snapshot: &mut PlanStateSnapshot, now_ms: u64) {
        snapshot.lane_history_count = self.lane_history_count;
        snapshot.undelivered_request_count = self.undelivered_requests;
        if self.delivery_uncertain {
            snapshot.planner_recovery_action = Some("resolve_delivery_uncertain".into());
        }
        snapshot.mission_control.event_log = self.event_log.clone();
        snapshot.mission_control.bounces = self.bounces.clone();
        let held = &mut snapshot.mission_control.held;
        for review in &mut held.held_reviews {
            let (count, at) = self
                .held_reviews
                .get(&review.task_id)
                .copied()
                .unwrap_or_default();
            review.verdicts = count;
            review.last_verdict_at = at;
            review.age_ms = at.map(|at| now_ms.saturating_sub(at));
        }
        held.verdicts_on_held = held.held_reviews.iter().map(|review| review.verdicts).sum();
        held.oldest_hold_age_ms = held
            .held_reviews
            .iter()
            .filter_map(|review| review.age_ms)
            .max();
    }
}

impl PlanStore {
    pub fn curator_schedule_projection(
        &self,
        plan_id: &str,
    ) -> Result<CuratorScheduleRead, PlanError> {
        let plan = self.database.curator_schedule_projection(plan_id)?;
        #[cfg(any(test, feature = "test-support"))]
        self.curator_schedule_reads.lock().push(plan_id.to_string());
        Ok(CuratorScheduleRead { plan })
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn curator_schedule_reads_for_test(&self) -> Vec<String> {
        self.curator_schedule_reads.lock().clone()
    }

    pub fn status_projection(
        &self,
        plan_id: &str,
        now_ms: u64,
    ) -> Result<PlanStatusRead, PlanError> {
        self.database.status_projection(plan_id, now_ms, &[])
    }
}
