use super::*;
fn event(id: &str, state: BackgroundWorkState) -> BackgroundWorkObservation {
    BackgroundWorkObservation {
        id: id.into(),
        state,
    }
}
#[test]
fn restart_guard_roster_drains_and_ignores_replayed_starts() {
    let mut roster = BackgroundRoster::default();
    roster.observe(&event("shell", BackgroundWorkState::Running), "fixture");
    assert!(!roster.is_empty());
    roster.observe(&event("shell", BackgroundWorkState::Completed), "");
    roster.observe(&event("shell", BackgroundWorkState::Running), "fixture");
    assert!(roster.is_empty());
    roster.observe(&event("agent", BackgroundWorkState::Running), "");
    roster.observe(&event("agent", BackgroundWorkState::Killed), "");
    assert!(roster.is_empty());
}
#[test]
fn restart_guard_unobserved_history_refuses_until_initialized() {
    let mut roster = BackgroundRoster::default();
    assert_eq!(roster.guarded_items().len(), 1);
    roster.history_initialized = true;
    assert!(roster.guarded_items().is_empty());
}
#[test]
fn restart_guard_incomplete_history_fails_closed() {
    let mut roster = BackgroundRoster::default();
    roster.history_incomplete = true;
    assert!(!roster.is_empty());
    assert_eq!(roster.items().len(), 1);
}
#[test]
fn restart_guard_roster_bound_fails_closed_and_tabs_are_independent() {
    let mut first = BackgroundRoster::default();
    for index in 0..=LIMIT {
        first.observe(&event(&index.to_string(), BackgroundWorkState::Running), "");
    }
    assert_eq!(first.running.len(), LIMIT);
    assert!(first.overflow);
    assert!(!first.is_empty());
    assert!(BackgroundRoster::default().is_empty());
    first = BackgroundRoster::default();
    assert!(first.is_empty());
}

#[test]
fn invalidated_history_does_not_revive_after_replay() {
    let mut roster = BackgroundRoster::default();
    roster.observe(&event("task", BackgroundWorkState::Unknown), "Task");
    roster.invalidate();
    roster.observe(&event("task", BackgroundWorkState::Running), "Task");
    assert!(roster.running.is_empty());
    roster.observe(&event("task", BackgroundWorkState::Completed), "");
    assert!(!roster.unknown.contains_key("task"));
    assert_eq!(roster.ended.get("task").map(String::as_str), Some("Task"));
}
