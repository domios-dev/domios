use crate::agent::test_provider::MockProvider;
use crate::multiplexer::state::MultiplexerState;
use crate::service::boot_stage::BootStage;
use crate::service::launch_diagnostics::AgentLaunchOrigin;
use crate::service::test_fixture::ServiceFixture;

const PROVIDER: &str = "restore-cwd-fixture";
const SESSION: &str = "restore-cwd-session";

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn restore_missing_cwd_marks_error() {
    let project = tempfile::tempdir().unwrap();
    let vanished = project.path().join("moved-away");
    let vanished_path = vanished.display().to_string();
    let (mut mux, tab_id) = MultiplexerState::new();
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some(PROVIDER.into());
        tab.agent_session_id = Some(SESSION.into());
        tab.cwd = Some(vanished_path.clone());
        tab.restored_from_snapshot = true;
        tab.agent_booting = true;
    }
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(MockProvider::with_session(PROVIDER, SESSION)));

    let (ready_tx, mut ready_rx) = tokio::sync::oneshot::channel();
    crate::service::orchestrator::launch_agent_in_background(
        fixture.service.clone(),
        tab_id.clone(),
        format!("{PROVIDER}-cli --resume {SESSION}"),
        PROVIDER.into(),
        AgentLaunchOrigin::RestoredSnapshot,
        None,
        None,
        None,
        Some(ready_tx),
    );

    let refusal = ready_rx
        .try_recv()
        .expect("the refusal is decided before any launch thread starts")
        .expect_err("a vanished cwd refuses the restored launch");
    assert!(refusal.contains(&vanished_path), "{refusal}");
    let tab = fixture.service.mux.read().get_tab(&tab_id).unwrap().clone();
    assert_eq!(tab.boot_stage, Some(BootStage::SpawnFailed));
    assert!(tab
        .boot_failure_reason
        .as_deref()
        .is_some_and(|reason| reason.contains(&vanished_path)));
    assert!(!tab.agent_booting);
    assert_eq!(tab.pty_id, None, "no PTY was started in another directory");
    assert_eq!(tab.agent_session_id.as_deref(), Some(SESSION));

    std::fs::create_dir(&vanished).unwrap();
    assert_eq!(
        super::refuse_launch_without_cwd(&fixture.service, &tab_id),
        None,
        "the same Tab launches once its directory exists again"
    );
}
