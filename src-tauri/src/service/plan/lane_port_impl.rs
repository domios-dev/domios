use super::ports::PlanLaneStatePort;
use crate::multiplexer::state::MultiplexerState;

impl PlanLaneStatePort for MultiplexerState {
    fn repoint_parent(&mut self, pane_id: &str, new_parent_id: &str) -> bool {
        self.repoint_parent(pane_id, new_parent_id)
    }
}
