use super::{PlanError, PlanStore};
use crate::plan::Plan;
use serde_json::Value;

pub struct BoardStateRead {
    pub plan: Plan,
    pub complete: bool,
    pub value: Value,
}

impl PlanStore {
    pub fn board_state_projection(
        &self,
        plan_id: &str,
        now_ms: u64,
        event_offset: usize,
        include_events: bool,
    ) -> Result<BoardStateRead, PlanError> {
        self.database
            .board_state_projection(plan_id, now_ms, event_offset, include_events)
    }
}
