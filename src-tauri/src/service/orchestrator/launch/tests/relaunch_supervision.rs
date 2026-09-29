use super::LaunchGeneration;
use crate::multiplexer::state::MultiplexerState;
use crate::service::launch_diagnostics::AgentLaunchOrigin;

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn restore_resumes_idle_worker() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let pty_id = "restored-worker-pty";
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.role = Some(crate::multiplexer::types::AgentRole::Worker);
        tab.agent_type = Some("restored-worker-fixture".into());
        tab.agent_session_id = Some("restored-worker-session".into());
        tab.pty_id = Some(pty_id.into());
        tab.restored_from_snapshot = true;
        tab.task_binding = Some(crate::multiplexer::types::TaskBinding {
            plan_id: Some("plan-restored".into()),
            task_id: Some("task-restored".into()),
            attempt_id: Some("attempt-restored".into()),
            spawn_handle: None,
            bound_at: 1,
        });
    }
    let harness = crate::service::test_fixture::ServiceFixture::new(mux);

    for origin in [
        AgentLaunchOrigin::RestoredSnapshot,
        AgentLaunchOrigin::Resume,
        AgentLaunchOrigin::Restart,
        AgentLaunchOrigin::ExitRecovery,
        AgentLaunchOrigin::AutomaticExitRecovery,
    ] {
        let mut launch = LaunchGeneration::begin(&tab_id, pty_id.into());
        launch
            .capture_binding_nonce(&harness.service, None, None, origin)
            .unwrap_or_else(|error| panic!("{origin:?} relaunch of a settled Worker: {error}"));
        let current = harness
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .and_then(|tab| tab.session_binding_nonce.clone());
        assert_eq!(current, launch.binding_nonce);
    }

    let mut dispatch = LaunchGeneration::begin(&tab_id, pty_id.into());
    assert!(dispatch
        .capture_binding_nonce(
            &harness.service,
            None,
            None,
            AgentLaunchOrigin::OrchestratedSpawn,
        )
        .is_err());
}
