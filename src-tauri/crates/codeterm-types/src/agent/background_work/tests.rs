use super::*;
fn event(id: &str, state: BackgroundWorkState) -> BackgroundWorkObservation {
    BackgroundWorkObservation {
        id: id.into(),
        state,
        ..Default::default()
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

#[test]
fn tool_call_observations_label_matching_tasks_without_overflow_or_visible_churn() {
    let mut roster = BackgroundRoster::default();
    let baseline = roster.clone();
    roster.observe(
        &BackgroundWorkObservation {
            id: "opaque-call-1".into(),
            state: BackgroundWorkState::ToolCall,
            kind: Some(BackgroundWorkKind::Shell),
            label: Some("opaque-cmd --run".into()),
            ..Default::default()
        },
        "",
    );
    assert!(roster.visible_eq(&baseline));
    assert!(roster.is_empty());

    roster.observe(
        &BackgroundWorkObservation {
            id: "opaque-task-1".into(),
            state: BackgroundWorkState::Running,
            kind: Some(BackgroundWorkKind::Shell),
            tool_use_id: Some("opaque-call-1".into()),
            ..Default::default()
        },
        "",
    );
    assert!(!roster.visible_eq(&baseline));
    assert_eq!(
        roster.running.get("opaque-task-1").map(String::as_str),
        Some("opaque-cmd --run")
    );

    // Direct observation label takes precedence when supplied.
    roster.observe(
        &BackgroundWorkObservation {
            id: "opaque-task-2".into(),
            state: BackgroundWorkState::Running,
            kind: Some(BackgroundWorkKind::Shell),
            label: Some("direct-cmd".into()),
            ..Default::default()
        },
        "",
    );
    assert_eq!(
        roster.running.get("opaque-task-2").map(String::as_str),
        Some("direct-cmd")
    );

    // Many foreground tool calls stay bounded and never trigger background overflow.
    for index in 0..=LIMIT + 10 {
        roster.observe(
            &BackgroundWorkObservation {
                id: format!("fg-call-{index}"),
                state: BackgroundWorkState::ToolCall,
                kind: Some(BackgroundWorkKind::Shell),
                label: Some(format!("fg-cmd-{index}")),
                ..Default::default()
            },
            "",
        );
    }
    assert!(!roster.overflow);
    assert_eq!(roster.running.len(), 2);
}
