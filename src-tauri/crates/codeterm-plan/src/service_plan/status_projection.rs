use super::state::input::StateInput;
use super::state::mission_control::{BounceSummary, EventLog};
use super::state::PlanStateSnapshot;
use super::{PlanError, PlanStore};
use std::collections::HashMap;

pub struct PlanNoticeRead {
    pub(super) connection: rusqlite::Connection,
    pub(super) plan_id: String,
    pub mode: crate::plan::PlanMode,
    pub owner_tab_id: Option<String>,
    pub team_id: Option<String>,
    pub complete: bool,
}

pub struct CuratorScheduleRead {
    pub(super) metadata: CuratorScheduleMetadata,
    pub(super) connection: rusqlite::Connection,
    pub(super) complete: bool,
    pub(super) group_close: Option<u64>,
}

#[derive(serde::Deserialize)]
pub struct CuratorScheduleMetadata {
    pub id: String,
    pub project_path: String,
    pub status: crate::plan::PlanStatus,
    pub mode: crate::plan::PlanMode,
    pub team_id: Option<String>,
    pub owner: crate::plan::OrchestratorRef,
    pub control_loop: crate::plan::ControlLoop,
}

#[derive(serde::Deserialize)]
pub struct CuratorAuthorityRead {
    pub owner: crate::plan::OrchestratorRef,
    pub running: Option<crate::plan::CuratorRun>,
}

impl CuratorScheduleRead {
    pub fn metadata(&self) -> &CuratorScheduleMetadata {
        &self.metadata
    }
    pub fn complete(&self) -> bool {
        self.complete
    }
    pub fn group_close(&self) -> Option<u64> {
        self.group_close
    }
    pub fn eligible(&self, enabled: bool, human_wait: bool) -> bool {
        super::curator::EligibilityInput {
            team_bound: self.metadata.team_id.is_some(),
            status: &self.metadata.status,
            mode: &self.metadata.mode,
            complete: self.complete,
            control_mode: &self.metadata.control_loop.mode,
        }
        .eligible(enabled, human_wait)
    }
}

pub struct PlanStatusRead {
    pub(super) revision: String,
    pub(super) input: StateInput,
    pub(super) event_log: EventLog,
    pub(super) bounces: BounceSummary,
    pub(super) held_reviews: HashMap<String, (usize, Option<u64>)>,
    pub(super) lane_history_count: usize,
    pub(super) undelivered_requests: usize,
    pub(super) delivery_uncertain: bool,
}

impl PlanStatusRead {
    pub fn input(&self) -> &StateInput {
        &self.input
    }

    pub fn apply_history_facts(&self, snapshot: &mut PlanStateSnapshot, now_ms: u64) {
        snapshot.revision = self.revision.clone();
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
    pub fn curator_authority_projection(
        &self,
        plan_id: &str,
    ) -> Result<CuratorAuthorityRead, PlanError> {
        self.database.curator_authority_projection(plan_id)
    }

    pub fn notice_projection(&self, plan_id: &str) -> Result<PlanNoticeRead, PlanError> {
        self.database.notice_projection(plan_id)
    }

    pub fn curator_schedule_projection(
        &self,
        plan_id: &str,
    ) -> Result<CuratorScheduleRead, PlanError> {
        let read = self.database.curator_schedule_projection(plan_id)?;
        #[cfg(any(test, feature = "test-support"))]
        self.curator_schedule_reads.lock().push(plan_id.to_string());
        Ok(read)
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
