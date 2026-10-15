use super::*;
use crate::multiplexer::types::RestorableLaunchParameters;

#[test]
fn saved_opaque_arguments_refuse_runtime_use_with_a_visible_reason_without_leaking_arguments() {
    let launch = RestorableLaunchParameters {
        args: Some(vec!["opaque-private-value".into()]),
        ..Default::default()
    };
    let failure = validate_restored_arguments("fixture-provider", Some(&launch)).unwrap_err();
    assert!(matches!(failure, RuntimeFailure::Refused(_)));
    let mut state = RuntimeState::unloaded(Some("saved-thread".into()));
    let epoch = state.begin();
    assert!(state.degrade(epoch, &failure));
    let snapshot = state.snapshot();
    assert_eq!(snapshot.state, "degraded");
    let reason = snapshot.reason.unwrap();
    assert!(reason.contains("runtime_launch_arguments"));
    assert!(!reason.contains("opaque-private-value"));
}

#[test]
fn saved_typed_settings_and_empty_arguments_remain_eligible_for_runtime_use() {
    assert!(validate_restored_arguments("fixture-provider", None).is_ok());
    let launch = RestorableLaunchParameters {
        requested_model: Some("typed-model".into()),
        requested_reasoning_effort: Some("typed-effort".into()),
        args: Some(Vec::new()),
        ..Default::default()
    };
    assert!(validate_restored_arguments("fixture-provider", Some(&launch)).is_ok());
}

#[path = "tests/sync_send.rs"]
mod sync_send;

#[test]
fn bind_commit_emits_one_ready_event_with_booting_and_session_bound() {
    let (mut state, _root) = crate::multiplexer::state::MultiplexerState::new();
    let workspace = state.list_workspaces()[0].id.clone();
    let id = state.create_tab(&workspace).unwrap();
    let tab = state.get_tab_mut(&id).unwrap();
    tab.agent_booting = true;
    tab.session_binding_nonce = Some("nonce".into());
    let mut events = Vec::new();
    commit_runtime_binding(tab, "nonce", "thread", |bound| {
        events.push(crate::service::boot_stage::boot_stage_event(
            &id,
            bound,
            codeterm_types::pty::BootStage::AgentReady,
            None,
        ));
    })
    .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["stage"], "agent_ready");
    assert_eq!(events[0]["agent_booting"], false);
    assert_eq!(events[0]["session_bound"], true);
    assert_eq!(tab.agent_session_id.as_deref(), Some("thread"));
    let mut rejected_events = 0;
    assert!(commit_runtime_binding(tab, "stale", "thread", |_| rejected_events += 1).is_err());
    assert_eq!(rejected_events, 0);
}
