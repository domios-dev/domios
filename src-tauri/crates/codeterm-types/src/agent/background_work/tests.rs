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
fn observation_label_takes_precedence_and_preserves_mixed_version_state_set() {
    let mut roster = BackgroundRoster::default();
    roster.observe(
        &BackgroundWorkObservation {
            id: "opaque-task-1".into(),
            state: BackgroundWorkState::Running,
            kind: Some(BackgroundWorkKind::Shell),
            label: Some("direct-cmd --run".into()),
            tool_use_id: Some("opaque-call-1".into()),
            ..Default::default()
        },
        "fallback-cmd",
    );
    assert_eq!(
        roster.running.get("opaque-task-1").map(String::as_str),
        Some("direct-cmd --run")
    );

    // Fallback label fills an initially empty unknown entry without needing extra state variants.
    roster.observe(
        &BackgroundWorkObservation {
            id: "opaque-task-2".into(),
            state: BackgroundWorkState::Unknown,
            kind: Some(BackgroundWorkKind::Shell),
            ..Default::default()
        },
        "",
    );
    roster.observe(
        &BackgroundWorkObservation {
            id: "opaque-task-2".into(),
            state: BackgroundWorkState::Unknown,
            kind: Some(BackgroundWorkKind::Shell),
            ..Default::default()
        },
        "resolved-from-side-table",
    );
    assert_eq!(
        roster.unknown.get("opaque-task-2").map(String::as_str),
        Some("resolved-from-side-table")
    );

    // Older hosts without label/tool_use_id ignore optional fields on wire deserialization.
    #[derive(serde::Deserialize)]
    struct LegacyObservation {
        id: String,
        state: BackgroundWorkState,
        kind: Option<BackgroundWorkKind>,
    }
    let wire = serde_json::to_value(BackgroundWorkObservation {
        id: "opaque-task-3".into(),
        state: BackgroundWorkState::Running,
        kind: Some(BackgroundWorkKind::Shell),
        label: Some("opaque-cmd".into()),
        tool_use_id: Some("opaque-call-3".into()),
        output_path: None,
    })
    .unwrap();
    let legacy: LegacyObservation = serde_json::from_value(wire).unwrap();
    assert_eq!(legacy.id, "opaque-task-3");
    assert_eq!(legacy.state, BackgroundWorkState::Running);
    assert_eq!(legacy.kind, Some(BackgroundWorkKind::Shell));
}
