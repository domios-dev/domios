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

#[path = "tests/capacity_retry.rs"]
mod capacity_retry;

#[path = "tests/continuation.rs"]
mod continuation;

#[path = "tests/rejected_before_write.rs"]
mod rejected_before_write;

#[path = "tests/bootstrap_ownership.rs"]
mod bootstrap_ownership;

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

async fn live_stop_fixture() -> (
    crate::service::test_fixture::ServiceFixture,
    String,
    test_support::LiveRuntime,
) {
    let (mut mux, _root) = crate::multiplexer::state::MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let id = mux.create_tab(&workspace).unwrap();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("fixture".into());
    tab.agent_session_id = Some("thread".into());
    tab.session_binding_nonce = Some("nonce".into());
    let prepared = tab.clone();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let live = tokio::task::spawn_blocking(move || test_support::live_runtime(&prepared))
        .await
        .unwrap();
    (fixture, id, live)
}

#[tokio::test]
async fn stop_awaits_tree_exit_on_current_thread_executor_without_nested_block_on() {
    let (fixture, id, live) = live_stop_fixture().await;
    let session = for_tab(&id).unwrap();
    let process = session.process.lock().clone().unwrap();
    assert!(stop_blocking(&fixture.service, &id).is_err());
    assert!(for_tab(&id).is_some());
    stop(&fixture.service, &id).await.unwrap();
    assert!(process.terminated());
    assert!(for_tab(&id).is_none());
    drop(live);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn synchronous_stop_bridge_releases_the_tokio_worker_and_awaits_exit() {
    let (fixture, id, live) = live_stop_fixture().await;
    let session = for_tab(&id).unwrap();
    let process = session.process.lock().clone().unwrap();
    stop_blocking(&fixture.service, &id).unwrap();
    assert!(process.terminated());
    assert!(for_tab(&id).is_none());
    drop(live);
}
