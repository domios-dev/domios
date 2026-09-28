use super::*;
use crate::multiplexer::state::MultiplexerState;

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn remote_task_bound_startup_accepts_matching_identity_without_local_supervisor() {
    let (mut mux, _) = MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let child_tab_id = mux.create_tab(&workspace_id).unwrap();
    let identity = StartupTaskIdentity {
        plan_id: "plan-remote".into(),
        task_id: "task-remote".into(),
        attempt_id: "attempt-remote".into(),
    };
    let child = mux.get_tab_mut(&child_tab_id).unwrap();
    child.pty_id = Some("pty-remote".into());
    child.session_binding_nonce = Some("nonce-remote".into());
    child.report_target_mesh_peer = Some("origin-peer".into());
    child.task_binding = Some(crate::multiplexer::types::TaskBinding {
        plan_id: Some(identity.plan_id.clone()),
        task_id: Some(identity.task_id.clone()),
        attempt_id: Some(identity.attempt_id.clone()),
        spawn_handle: None,
        bound_at: 1,
    });
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);

    fixture
        .service
        .activate_agent_startup_supervision(
            &child_tab_id,
            "pty-remote",
            "nonce-remote",
            Some(&identity),
        )
        .expect("remote task launch does not require a local supervisor tab");
    assert!(fixture.service.worker_ack.startup_waiters.lock().is_empty());
    let mut mismatched = identity;
    mismatched.attempt_id = "another-attempt".into();
    assert!(fixture
        .service
        .activate_agent_startup_supervision(
            &child_tab_id,
            "pty-remote",
            "nonce-remote",
            Some(&mismatched),
        )
        .unwrap_err()
        .contains("no supervised startup waiter"));
}
