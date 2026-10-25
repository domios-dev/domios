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
        }],
        ..Default::default()
    };
    let roster =
        observe_background_records(&BackgroundRoster::default(), false, "own", &[record], true);
    assert_eq!(
        roster.running.get("opaque-id").map(String::as_str),
        Some("Inspect source")
    );
}
#[test]
fn restart_guard_refuses_named_work_and_allows_explicit_force() {
    let items = vec!["shell-fixture".into(), "agent-fixture".into()];
    assert!(
        matches!(restart_guard(items.clone(),false),Err(super::super::agent_restart::AgentRestartError::BackgroundWork{items:running}) if running==items)
    );
    assert!(restart_guard(items, true).is_ok());
    assert!(restart_guard(Vec::new(), false).is_ok());
}
#[test]
fn background_record_compute_keeps_fork_and_session_filters_and_completion_dedupe() {
    use crate::agent::chat_types::RichMessage;
    use codeterm_types::agent::background_work::BackgroundRoster;
    let record = |session: Option<&str>, state| RichMessage {
        provider_session_id: session.map(str::to_string),
        background_work: vec![BackgroundWorkObservation {
            id: "work".into(),
            state,
        }],
        ..Default::default()
    };
    let before = BackgroundRoster::default();
    let foreign = record(Some("foreign"), BackgroundWorkState::Running);
    let unbound = record(None, BackgroundWorkState::Running);
    assert_eq!(
        observe_background_records(&before, false, "own", &[foreign], true),
        before
    );
    assert_eq!(
        observe_background_records(&before, true, "own", &[unbound.clone()], true),
        before
    );
    let running = observe_background_records(&before, false, "own", &[unbound], true);
    assert_eq!(running.running.len(), 1);
    assert!(before.running.is_empty());
    assert_eq!(
        observe_background_records(
            &running,
            false,
            "own",
            &[record(Some("foreign"), BackgroundWorkState::Killed)],
            true
        ),
        running
    );
    let stopped = observe_background_records(
        &running,
        false,
        "own",
        &[record(Some("own"), BackgroundWorkState::Killed)],
        true,
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
        }],
        ..Default::default()
    };
    let historical = observe_background_records(
        &BackgroundRoster::default(),
        false,
        "own",
        &[record.clone()],
        false,
    );
    assert!(historical.running.is_empty());
    assert!(historical.unknown.contains_key("task"));
    let live = observe_background_records(&historical, false, "own", &[record], true);
    assert!(live.running.contains_key("task"));
    let boundary = crate::agent::chat_types::RichMessage {
        provider_session_id: Some("own".into()),
        background_work: vec![BackgroundWorkObservation {
            id: String::new(),
            state: BackgroundWorkState::SessionBoundary,
        }],
        ..Default::default()
    };
    let invalidated = observe_background_records(&live, false, "own", &[boundary], true);
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
        },
        "",
    );
    let old_boundary = crate::agent::chat_types::RichMessage {
        provider_session_id: Some("own".into()),
        background_work: vec![BackgroundWorkObservation {
            id: String::new(),
            state: BackgroundWorkState::SessionBoundary,
        }],
        ..Default::default()
    };
    assert_eq!(
        observe_background_records(&current, false, "own", &[old_boundary], false),
        current
    );
}
