use super::{PlanError, PlanStore};
use crate::plan::Plan;

pub struct CuratorDigestRead {
    pub(super) connection: rusqlite::Connection,
    pub(super) plan: Plan,
    pub(super) since: u64,
    pub(super) real_rounds: u64,
    pub(super) latest_handoff: Option<u64>,
}

impl CuratorDigestRead {
    pub fn plan(&self) -> &Plan {
        &self.plan
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
