use crate::agent::offline_state_classifier::{AgentLiveState, TimelineState};

/// The finished turn's summary scrolls off screen while the agent stays idle; it still describes that turn.
pub(super) fn carry_summary(previous: Option<&AgentLiveState>, next: &mut AgentLiveState) {
    let Some(previous) = previous else { return };
    if previous.state != TimelineState::Idle
        || next.state != TimelineState::Idle
        || next.last_turn_summary.is_some()
    {
        return;
    }
    next.last_turn_summary = previous.last_turn_summary.clone();
    next.elapsed_ms = previous.elapsed_ms;
}

#[cfg(test)]
#[path = "finished_turn/tests.rs"]
mod tests;
