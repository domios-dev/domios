use serde::{Deserialize, Serialize};

/// An Active plan whose Team closed while tasks were unfinished. It stays
/// visible and ownerless until a live Team adopts it; it is never auto-archived.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanOrphan {
    pub team_id: String,
    pub reason: String,
    pub unfinished_task_ids: Vec<String>,
    pub at: u64,
}

impl super::Plan {
    pub fn is_orphaned_active(&self) -> bool {
        self.status == super::PlanStatus::Active
            && self.team_id.is_none()
            && self.orphaned.is_some()
    }
}
