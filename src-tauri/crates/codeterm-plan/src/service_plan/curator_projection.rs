use super::state::TaskCounts;
use super::{PlanError, PlanStore};
mod inputs;
pub use inputs::{CuratorDigestScope, CuratorRoundCursorRead, CuratorRuntimeNotice};

pub struct CuratorDigestRead {
    pub(super) connection: rusqlite::Connection,
    pub(super) scope: CuratorDigestScope,
    pub(super) now: u64,
    pub(super) counts: TaskCounts,
    pub(super) lanes: usize,
    pub(super) group_close: Option<u64>,
    pub(super) since: u64,
    pub(super) real_rounds: u64,
    pub(super) latest_handoff: Option<u64>,
}

impl CuratorDigestRead {
    pub fn scope(&self) -> &CuratorDigestScope {
        &self.scope
    }
    pub fn counts(&self) -> &TaskCounts {
        &self.counts
    }
    pub fn lanes(&self) -> usize {
        self.lanes
    }
    pub fn group_close(&self) -> Option<u64> {
        self.group_close
    }
    pub fn complete(&self) -> bool {
        self.counts.total > 0 && self.counts.done + self.counts.cancelled == self.counts.total
    }
    pub fn eligible(&self, enabled: bool, human_wait: bool) -> bool {
        super::curator::EligibilityInput {
            team_bound: self.scope.team_id.is_some(),
            status: &self.scope.status,
            mode: &self.scope.mode,
            complete: self.complete(),
            control_mode: &self.scope.control_mode,
        }
        .eligible(enabled, human_wait)
    }
    pub fn synthesis_context_header(&self, base: &str) -> Result<String, PlanError> {
        super::store_db::curator_input_queries::synthesis_context(self, base)
    }
    pub fn heartbeat_body(&self) -> Result<String, PlanError> {
        super::store_db::curator_input_queries::heartbeat_body(self)
    }
    pub fn has_actionable(&self) -> Result<bool, PlanError> {
        super::store_db::curator_input_queries::has_actionable(self)
    }
    pub fn since(&self) -> u64 {
        self.since
    }
    pub fn real_rounds_started(&self) -> u64 {
        self.real_rounds
    }
    pub fn latest_handoff_at(&self) -> Option<u64> {
        self.latest_handoff
    }
}

impl PlanStore {
    pub fn curator_digest_projection(
        &self,
        id: &str,
        now: u64,
    ) -> Result<CuratorDigestRead, PlanError> {
        self.database.curator_digest_projection(id, now)
    }
}
