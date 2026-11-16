use super::*;
use crate::multiplexer::types::RestorableLaunchParameters;

#[test]
fn close_reservation_prevents_runtime_registration() {
    let (fixture, tab_id, _peer, cleanup) = sync_send::runtime_fixture();
    drop(cleanup);
    assert!(for_tab(&tab_id).is_none());
    let _reservation = fixture
        .service
        .mux
        .write()
        .reserve_tab_close(&tab_id)
        .unwrap();
    let error = crate::async_runtime::block_on(ensure(&fixture.service, &tab_id))
        .err()
        .unwrap();
    assert!(
        matches!(error, RuntimeFailure::Refused(ref reason) if reason.contains("Tab is closing"))
    );
    assert!(for_tab(&tab_id).is_none());
}

#[test]
fn restricted_cli_trust_projection_never_exposes_project_path() {
    let projected = operation::policy_projection(&serde_json::json!({
        "cwd":"/opaque/private-project",
        "config":{"projects":{"/opaque/private-project":{"trust_level":"trusted"}}}
    }));
    assert_eq!(projected["top"]["projectTrust"], "trusted");
    assert!(!projected.to_string().contains("private-project"));
}

#[test]
fn runtime_policy_wire_projection_preserves_precedence_without_private_values() {
    let raw = serde_json::json!({
        "mode":"read-only",
        "approvalPolicy":"never",
        "sandbox":{"type":"readOnly","writableRoots":["private-root"]},
        "activePermissionProfile":{"id":"private-profile"},
        "thread":{"mode":"private-thread-mode"},
        "threadSettings":{
            "mode":"workspace-write",
            "approvalPolicy":"never",
            "approvalsReviewer":"user",
            "sandbox":{"type":"readOnly"},
            "sandboxPolicy":{"type":"workspaceWrite"},
            "activePermissionProfile":{"id":":workspace"},
            "thread":{"mode":"read-only","sandbox":{"type":"dangerFullAccess"}},
            "config":{"sandbox_mode":"private-config"},
            "developerInstructions":"private-body"
        }
    });
    let projected = operation::policy_projection(&raw);
    assert_eq!(projected["threadSettingsSelected"], true);
    assert_eq!(projected["top"]["mode"], "read-only");
    assert_eq!(projected["top"]["sandbox"], "readOnly");
    assert_eq!(projected["top"]["profile"], "other");
    assert_eq!(projected["topThread"]["mode"], "other");
    assert_eq!(projected["selected"]["mode"], "workspace-write");
    assert_eq!(projected["selected"]["sandbox"], "readOnly");
    assert_eq!(projected["selected"]["sandboxPolicy"], "workspaceWrite");
    assert_eq!(projected["selected"]["approval"], "never");
    assert_eq!(projected["selected"]["profile"], ":workspace");
    assert_eq!(projected["selected"]["configSandbox"], "other");
    assert_eq!(projected["selectedThread"]["mode"], "read-only");
    assert_eq!(projected["selectedThread"]["sandbox"], "dangerFullAccess");
    assert!(!projected.to_string().contains("private"));
    let request = operation::policy_projection(&serde_json::json!({
        "sandbox":"workspace-write","approvalPolicy":"never",
        "config":{"sandbox_mode":"read-only"},"permissions":"private-permissions"
    }));
    assert_eq!(request["top"]["sandbox"], "workspace-write");
    assert_eq!(request["top"]["configSandbox"], "read-only");
    assert_eq!(request["top"]["permissions"], "other");
    for nested in [
        Value::Null,
        Value::Bool(false),
        serde_json::json!(0),
        serde_json::json!(""),
    ] {
        let projected = operation::policy_projection(
            &serde_json::json!({"mode":"workspace-write","threadSettings":nested}),
        );
        assert_eq!(projected["threadSettingsSelected"], false);
        assert_eq!(projected["selected"]["mode"], "workspace-write");
    }
    let malformed = operation::policy_projection(
        &serde_json::json!({"mode":"workspace-write","threadSettings":"private-truthy"}),
    );
    assert_eq!(malformed["threadSettingsSelected"], true);
    assert_eq!(malformed["selected"]["mode"], "missing");
    assert!(!malformed.to_string().contains("private"));
}

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
pub(super) mod sync_send;

#[path = "tests/continuation.rs"]
mod continuation;

#[path = "tests/rejected_before_write.rs"]
mod rejected_before_write;

#[path = "tests/sign_in.rs"]
mod sign_in;

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
    commit_runtime_binding(tab, "nonce", "thread", true, |bound| {
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
    assert_eq!(
        tab.agent_runtime_config.as_ref().unwrap()["unusedThreadId"],
        "thread"
    );
    assert_eq!(tab.agent_session_id.as_deref(), Some("thread"));
    let mut rejected_events = 0;
    assert!(
        commit_runtime_binding(tab, "stale", "thread", true, |_| rejected_events += 1).is_err()
    );
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

#[path = "tests/history_ready.rs"]
mod history_ready;

#[path = "tests/capacity_retry.rs"]
mod capacity_retry;
