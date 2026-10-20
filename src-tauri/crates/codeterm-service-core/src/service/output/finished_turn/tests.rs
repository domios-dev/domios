use super::carry_summary;
use crate::agent::offline_state_classifier::{AgentLiveState, TabVerdict, TimelineState};

fn state(
    timeline: TimelineState,
    summary: Option<&str>,
    elapsed_ms: Option<u64>,
) -> AgentLiveState {
    let mut state = AgentLiveState::from(TabVerdict {
        state: timeline,
        subtype: None,
    });
    state.last_turn_summary = summary.map(str::to_string);
    state.elapsed_ms = elapsed_ms;
    state
}

#[test]
fn idle_repaint_without_the_summary_keeps_the_finished_turn() {
    let previous = state(TimelineState::Idle, Some("fixture-summary"), Some(9_000));
    let mut next = state(TimelineState::Idle, None, None);
    carry_summary(Some(&previous), &mut next);
    assert_eq!(next.last_turn_summary.as_deref(), Some("fixture-summary"));
    assert_eq!(next.elapsed_ms, Some(9_000));
}

#[test]
fn a_new_summary_replaces_the_carried_one() {
    let previous = state(TimelineState::Idle, Some("fixture-old"), Some(9_000));
    let mut next = state(TimelineState::Idle, Some("fixture-new"), Some(3_000));
    carry_summary(Some(&previous), &mut next);
    assert_eq!(next.last_turn_summary.as_deref(), Some("fixture-new"));
    assert_eq!(next.elapsed_ms, Some(3_000));
}

#[test]
fn a_turn_that_just_finished_starts_without_a_carried_summary() {
    let previous = state(TimelineState::Working, None, Some(4_000));
    let mut next = state(TimelineState::Idle, None, None);
    carry_summary(Some(&previous), &mut next);
    assert_eq!(next.last_turn_summary, None);
    assert_eq!(next.elapsed_ms, None);
}
