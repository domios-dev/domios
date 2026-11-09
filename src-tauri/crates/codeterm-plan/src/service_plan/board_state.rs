use super::ui_projections::board::PlanBoardState;
use super::{PlanError, PlanStore};
use crate::plan::{PlanMode, PlanStatus};

pub struct BoardStateRead {
    pub mode: PlanMode,
    pub status: PlanStatus,
    pub team_id: Option<String>,
    pub complete: bool,
    pub value: PlanBoardState,
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
