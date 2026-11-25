use super::*;
#[test]
fn background_roster_uses_provider_agent_description() {
    use crate::agent::chat_types::{MessageBlock, RichMessage};
    use codeterm_types::agent::background_work::BackgroundRoster;
    let record = RichMessage {
        provider_session_id: Some("own".into()),
        blocks: vec![MessageBlock {
            kind: "tool_use".into(),
            data: serde_json::json!({
                "tool_kind": "agent", "input": { "agent_id": "opaque-id", "agent_description": "Inspect source" }
            }),
        }],
        background_work: vec![BackgroundWorkObservation {
            id: "opaque-id".into(),
            state: BackgroundWorkState::Running,
            ..Default::default()
        }],
        ..Default::default()
    };
    let roster = observe_background_records(
        &BackgroundRoster::default(),
        false,
        "own",
        &[record],
        true,
        &HashMap::new(),
    );
    assert_eq!(
        roster.running.get("opaque-id").map(String::as_str),
        Some("Inspect source")
    );
}
#[test]
fn restart_guard_refuses_named_work_and_allows_explicit_force() {
    let items = vec!["shell-fixture".into(), "agent-fixture".into()];
    assert!(
        matches!(restart_guard(items.clone(),false),Err(super::super::agent_lifecycle::AgentRestartError::BackgroundWork{items:running}) if running==items)
    );
    assert!(restart_guard(items, true).is_ok());
    assert!(restart_guard(Vec::new(), false).is_ok());
}
#[test]
fn background_record_compute_keeps_fork_and_session_filters_and_completion_dedupe() {
    use crate::agent::chat_types::RichMessage;
    use codeterm_types::agent::background_work::{BackgroundRoster, BackgroundWorkKind};
    let record = |session: Option<&str>, state| RichMessage {
        provider_session_id: session.map(str::to_string),
        background_work: vec![BackgroundWorkObservation {
            id: "work".into(),
            state,
            kind: Some(BackgroundWorkKind::Shell),
            output_path: (state == BackgroundWorkState::Running).then(|| "opaque-log".into()),
            ..Default::default()
        }],
        ..Default::default()
    };
    let before = BackgroundRoster::default();
    let foreign = record(Some("foreign"), BackgroundWorkState::Running);
    let unbound = record(None, BackgroundWorkState::Running);
    let empty = HashMap::new();
    assert_eq!(
        observe_background_records(&before, false, "own", &[foreign], true, &empty),
        before
    );
    assert_eq!(
        observe_background_records(&before, true, "own", &[unbound.clone()], true, &empty),
        before
    );
    let running = observe_background_records(&before, false, "own", &[unbound], true, &empty);
    assert_eq!(running.running.len(), 1);
    assert_eq!(running.output_path("work"), None);
    assert!(before.running.is_empty());
    let running = observe_background_records(
        &running,
        false,
        "own",
        &[record(Some("own"), BackgroundWorkState::Running)],
        true,
        &empty,
    );
    assert_eq!(running.output_path("work"), Some("opaque-log"));
    assert_eq!(
        observe_background_records(
            &running,
            false,
            "own",
            &[record(Some("foreign"), BackgroundWorkState::Killed)],
            true,
            &empty,
        ),
        running
    );
    let stopped = observe_background_records(
        &running,
        false,
        "own",
        &[record(Some("own"), BackgroundWorkState::Killed)],
        true,
        &empty,
    );
    assert!(stopped.running.is_empty());
    assert!(stopped.ended.contains_key("work"));
    let completed = observe_background_records(
        &running,
        true,
        "own",
        &[
            record(Some("own"), BackgroundWorkState::Completed),
            record(Some("own"), BackgroundWorkState::Running),
        ],
        true,
        &empty,
    );
    assert!(completed.running.is_empty());
}

#[test]
fn background_history_is_unknown_and_live_append_is_running() {
    use codeterm_types::agent::background_work::BackgroundRoster;
    let record = crate::agent::chat_types::RichMessage {
        provider_session_id: Some("own".into()),
        background_work: vec![BackgroundWorkObservation {
            id: "task".into(),
            state: BackgroundWorkState::Running,
            ..Default::default()
        }],
        ..Default::default()
    };
    let empty = HashMap::new();
    let historical = observe_background_records(
        &BackgroundRoster::default(),
        false,
        "own",
        &[record.clone()],
        false,
        &empty,
    );
    assert!(historical.running.is_empty());
    assert!(historical.unknown.contains_key("task"));
    let live = observe_background_records(&historical, false, "own", &[record], true, &empty);
    assert!(live.running.contains_key("task"));
    let boundary = crate::agent::chat_types::RichMessage {
        provider_session_id: Some("own".into()),
        background_work: vec![BackgroundWorkObservation {
            id: String::new(),
            state: BackgroundWorkState::SessionBoundary,
            ..Default::default()
        }],
        ..Default::default()
    };
    let invalidated = observe_background_records(&live, false, "own", &[boundary], true, &empty);
    assert!(invalidated.running.is_empty());
    assert!(invalidated.unknown.contains_key("task"));
}

#[test]
fn background_historical_boundary_cannot_invalidate_newer_live_work() {
    use codeterm_types::agent::background_work::BackgroundRoster;
    let mut current = BackgroundRoster::default();
    current.history_initialized = true;
    current.observe(
        &BackgroundWorkObservation {
            id: "live".into(),
            state: BackgroundWorkState::Running,
            ..Default::default()
        },
        "",
    );
    let old_boundary = crate::agent::chat_types::RichMessage {
        provider_session_id: Some("own".into()),
        background_work: vec![BackgroundWorkObservation {
            id: String::new(),
            state: BackgroundWorkState::SessionBoundary,
            ..Default::default()
        }],
        ..Default::default()
    };
    assert_eq!(
        observe_background_records(
            &current,
            false,
            "own",
            &[old_boundary],
            false,
            &HashMap::new()
        ),
        current
    );
}

#[test]
fn own_session_tool_call_side_table_labels_background_shell_without_touching_roster() {
    use crate::agent::chat_types::RichMessage;
    use codeterm_types::agent::background_work::{
        BackgroundRoster, BackgroundToolCall, BackgroundWorkKind,
    };
    let tab = "side-table-test-tab";
    clear_tab_labels(tab);
    let foreign_call = RichMessage {
        provider_session_id: Some("foreign".into()),
        background_tool_calls: vec![BackgroundToolCall {
            id: "opaque-call".into(),
            label: "foreign-cmd".into(),
            kind: Some(BackgroundWorkKind::Shell),
        }],
        ..Default::default()
    };
    let own_launch = RichMessage {
        provider_session_id: Some("own".into()),
        background_work: vec![BackgroundWorkObservation {
            id: "opaque-bg".into(),
            state: BackgroundWorkState::Running,
            kind: Some(BackgroundWorkKind::Shell),
            tool_use_id: Some("opaque-call".into()),
            ..Default::default()
        }],
        ..Default::default()
    };
    let (has_events, resolved) = record_and_resolve_tool_labels(
        tab,
        "own",
        "nonce-1",
        false,
        &[foreign_call, own_launch.clone()],
    );
    assert!(has_events);
    assert!(resolved.is_empty());
    let without_own_call = observe_background_records(
        &BackgroundRoster::default(),
        false,
        "own",
        &[own_launch.clone()],
        true,
        &resolved,
    );
    assert_eq!(
        without_own_call
            .running
            .get("opaque-bg")
            .map(String::as_str),
        Some("")
    );

    // Foreground tool call updates only the side table; no roster events -> no mux write / CAS contention.
    let own_call = RichMessage {
        provider_session_id: Some("own".into()),
        background_tool_calls: vec![BackgroundToolCall {
            id: "opaque-call".into(),
            label: "opaque-cmd 900".into(),
            kind: Some(BackgroundWorkKind::Shell),
        }],
        ..Default::default()
    };
    let (fg_has_events, fg_resolved) =
        record_and_resolve_tool_labels(tab, "own", "nonce-1", false, &[own_call]);
    assert!(!fg_has_events);
    assert!(fg_resolved.is_empty());

    // Subsequent launch resolves the command from the side table for the exact same (session, nonce).
    let (launch_has_events, launch_resolved) =
        record_and_resolve_tool_labels(tab, "own", "nonce-1", false, &[own_launch.clone()]);
    assert!(launch_has_events);
    let labeled = observe_background_records(
        &BackgroundRoster::default(),
        false,
        "own",
        &[own_launch.clone()],
        true,
        &launch_resolved,
    );
    assert_eq!(
        labeled.running.get("opaque-bg").map(String::as_str),
        Some("opaque-cmd 900")
    );
    assert_eq!(
        labeled.kinds.get("opaque-bg"),
        Some(&BackgroundWorkKind::Shell)
    );

    // Changing binding nonce resets the side table so stale call labels never leak across sessions.
    let (_, after_rebind) =
        record_and_resolve_tool_labels(tab, "own", "nonce-2", false, &[own_launch]);
    assert!(after_rebind.is_empty());

    // Bounded FIFO eviction for > TOOL_LABEL_LIMIT foreground calls.
    let many: Vec<RichMessage> = (0..=TOOL_LABEL_LIMIT + 10)
        .map(|index| RichMessage {
            provider_session_id: Some("own".into()),
            background_tool_calls: vec![BackgroundToolCall {
                id: format!("fg-call-{index}"),
                label: format!("fg-cmd-{index}"),
                kind: Some(BackgroundWorkKind::Shell),
            }],
            ..Default::default()
        })
        .collect();
    let (many_has_events, _) = record_and_resolve_tool_labels(tab, "own", "nonce-2", false, &many);
    assert!(!many_has_events);
    assert_eq!(
        tool_label_store().lock().get(tab).unwrap().labels.len(),
        TOOL_LABEL_LIMIT
    );
    clear_tab_labels(tab);
}
