use serde::{Deserialize, Serialize};

/// An Active plan whose Team closed while tasks were unfinished. It stays
/// visible and ownerless until a live Team adopts it; it is never auto-archived.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ts_rs::TS)]
pub struct PlanOrphan {
    pub team_id: String,
    pub reason: String,
    pub unfinished_task_ids: Vec<String>,
    pub at: u64,
}

impl super::Plan {
    pub fn is_orphaned_active(&self) -> bool {
        orphaned_active_for(
            &self.status,
            self.team_id.as_deref(),
            self.orphaned.as_ref(),
        )
    }
}

pub fn orphaned_active_for(
    status: &super::PlanStatus,
    team_id: Option<&str>,
    orphaned: Option<&PlanOrphan>,
) -> bool {
    *status == super::PlanStatus::Active && team_id.is_none() && orphaned.is_some()
}
