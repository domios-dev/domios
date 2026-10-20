use super::*;
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
        observe_background_records(&before, false, "own", &[foreign]),
        before
    );
    assert_eq!(
        observe_background_records(&before, true, "own", &[unbound.clone()]),
        before
    );
    let running = observe_background_records(&before, false, "own", &[unbound]);
    assert_eq!(running.running.len(), 1);
    assert!(before.running.is_empty());
    let completed = observe_background_records(
        &running,
        true,
        "own",
        &[
            record(Some("own"), BackgroundWorkState::Completed),
            record(Some("own"), BackgroundWorkState::Running),
        ],
    );
    assert!(completed.running.is_empty());
}
