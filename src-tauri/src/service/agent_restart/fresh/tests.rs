use super::*;
use crate::multiplexer::types::CloseRestoreTabState;

#[test]
fn fresh_restart_requires_force_for_live_agent() {
    assert!(fresh_restart_guard(true, false, false, Some(true)).is_err());
    assert!(fresh_restart_guard(true, true, false, Some(true)).is_ok());
}

#[test]
fn fresh_restart_never_replaces_existing_exact_session() {
    for force in [false, true] {
        assert!(fresh_restart_guard(false, force, true, Some(true)).is_err());
        assert!(fresh_restart_guard(false, force, true, None).is_err());
        assert!(fresh_restart_guard(false, force, true, Some(false)).is_ok());
    }
}

#[test]
fn fresh_restart_clears_binding_and_preserves_tab_identity() {
    let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.title = "Volta".into();
    tab.agent_type = Some("fixture-provider".into());
    tab.agent_session_id = Some("dead-session".into());
    tab.session_binding_nonce = Some("dead-nonce".into());
    tab.reported_session_binding_nonce = Some("dead-nonce".into());
    tab.cwd = Some("fixture-cwd".into());
    tab.team_id = Some("fixture-team".into());
    tab.role = Some(crate::multiplexer::types::AgentRole::Worker);
    tab.task_binding = Some(crate::multiplexer::types::TaskBinding {
        plan_id: Some("fixture-plan".into()),
        task_id: Some("fixture-task".into()),
        attempt_id: Some("fixture-attempt".into()),
        ..Default::default()
    });
    tab.restorable_launch.captured = true;
    tab.restorable_launch.model = Some("fixture-model".into());
    tab.restorable_launch.requested_reasoning_effort = Some("fixture-effort".into());
    tab.prior_session_binding_nonces
        .push(crate::multiplexer::types::PriorBindingNonce {
            session_id: Some("dead-session".into()),
            resume_target_session_id: Some("dead-session".into()),
            nonce: "prior-nonce".into(),
        });
    tab.restored_from_snapshot = true;
    tab.close_restore = Some(CloseRestoreTabState {
        exact_session: true,
        ..Default::default()
    });
    let recorded_launch = tab.restorable_launch.clone();
    let before = serde_json::to_value(&*tab).unwrap();
    clear_fresh_session_binding(tab);
    let after = serde_json::to_value(&*tab).unwrap();
    for field in ["id", "title", "role", "team_id", "cwd", "task_binding"] {
        assert_eq!(after[field], before[field], "{field}");
    }
    assert!(tab.agent_session_id.is_none());
    assert_eq!(tab.restorable_launch, recorded_launch);
    assert!(tab.session_binding_nonce.is_none());
    assert!(tab.reported_session_binding_nonce.is_none());
    assert!(!tab.close_restore.as_ref().unwrap().exact_session);
    assert!(!tab.restored_from_snapshot);
    assert!(tab.prior_session_binding_nonces.is_empty());
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn fresh_restart_service_uses_fresh_launch_and_rechecks_provider_store() {
    let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.agent_session_id = Some("dead-session".into());
    tab.cwd = Some("fixture-cwd".into());
    tab.boot_stage = Some(BootStage::SpawnFailed);
    tab.restored_from_snapshot = true;
    tab.close_restore = Some(CloseRestoreTabState {
        exact_session: true,
        ..Default::default()
    });
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.set_agent_full_access(false);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("fixture-provider"),
    ));
    let snap = fixture.service.fresh_restart_snapshot(&id, false).unwrap();
    let launch = super::super::relaunch::build_relaunch(&fixture.service, &snap, false).unwrap();
    assert!(!launch.command.contains("dead-session"));
    assert!(fixture.service.failed_launch_restart_snapshot(&id).is_err());
    assert!(fixture
        .service
        .require_exact_session_restart(&snap)
        .is_err());
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_session("fixture-provider", "dead-session"),
    ));
    assert!(fixture
        .service
        .require_fresh_restart_allowed(&snap, false)
        .is_err());
    assert!(matches!(
        fixture.service.agent_restart_observed_with_options(
            &id,
            super::super::RestartOptions {
                fresh: true,
                force: true,
                ..Default::default()
            },
        ),
        Err(RestartAttemptError::Request(AgentRestartError::BadRequest(
            _
        )))
    ));
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&id)
            .unwrap()
            .agent_session_id
            .as_deref(),
        Some("dead-session")
    );
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("fixture-provider"),
    ));
    fixture.service.commit_fresh_restart(&snap).unwrap();
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&id)
        .unwrap()
        .agent_session_id
        .is_none());
}
