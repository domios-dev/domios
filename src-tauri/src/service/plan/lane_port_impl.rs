use crate::multiplexer::state::MultiplexerState;
use codeterm_plan::service_plan::ports::PlanLaneStatePort;

pub struct LaneStatePort<'a>(pub &'a mut MultiplexerState);

impl PlanLaneStatePort for LaneStatePort<'_> {
    fn repoint_parent(&mut self, tab_id: &str, new_parent_id: &str) -> bool {
        self.0.repoint_parent(tab_id, new_parent_id)
    }
}
