use super::*;

#[test]
fn unconfirmed_general_launch_recovers_its_own_journaled_session_without_liveness() {
    use crate::multiplexer::types::PriorBindingNonce;
    use crate::session::lifecycle::LifecycleTransition;

    let mut mux = crate::multiplexer::state::MultiplexerState::empty();
    crate::service::general_agent::ensure_general_workspace(&mut mux, true);
    let id = mux
        .resolve_tab_id(crate::service::general_agent::GENERAL_ALIAS)
        .unwrap();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.cwd = Some("D:/fixture".into());
    tab.boot_stage = Some(BootStage::LaunchBindingUnconfirmed);
    tab.agent_booting = false;
    tab.prior_session_binding_nonces.push(PriorBindingNonce {
        nonce: "previous-nonce".into(),
        session_id: Some("owned-session".into()),
        resume_target_session_id: None,
    });
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.set_agent_full_access(false);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_session(
            "fixture-provider",
            "owned-session",
        ),
    ));
    fixture.service.open_lifecycle_epoch(&id).unwrap();
    fixture.service.record_lifecycle(
        &id,
        LifecycleTransition::SessionBound {
            session_id: "owned-session".into(),
            binding_nonce: "previous-nonce".into(),
            source: Default::default(),
        },
    );
    let recovery = fixture
        .service
        .failed_launch_restart_snapshot(&id)
        .unwrap()
        .unwrap();
    assert_eq!(recovery.session_id.as_deref(), Some("owned-session"));
    assert_eq!(recovery.provider, "fixture-provider");
    assert!(
        crate::service::general_agent::claim_general_agent_tab_retry(&id, &fixture.service.mux)
            .unwrap()
    );
}

#[test]
fn never_started_pty_can_restart_without_observed_agent_liveness() {
    for booting in [false, true] {
        for session in [None, Some("saved-session")] {
            let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
            let tab = mux.get_tab_mut(&id).unwrap();
            tab.protected = false;
            tab.restored_from_snapshot = true;
            tab.agent_type = Some("fixture-provider".into());
            tab.agent_session_id = session.map(str::to_owned);
            tab.cwd = Some("D:/fixture".into());
            tab.boot_stage = Some(BootStage::PtyStarting);
            tab.agent_booting = booting;
            let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
            fixture.set_agent_full_access(false);
            fixture.service.agent_registry.register_provider(Box::new(
                crate::agent::test_provider::MockProvider::with_session(
                    "fixture-provider",
                    "saved-session",
                ),
            ));
            let snap = fixture
                .service
                .failed_launch_restart_snapshot(&id)
                .unwrap()
                .unwrap();
            assert_eq!(snap.tab_id, id);
            assert_eq!(snap.provider, "fixture-provider");
            assert_eq!(snap.session_id.as_deref(), session);
            assert!(snap.pty_id.is_empty());
        }
    }
}

#[test]
fn never_started_pty_cannot_replace_a_missing_saved_session() {
    let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.agent_session_id = Some("missing-session".into());
    tab.cwd = Some("D:/fixture".into());
    tab.boot_stage = Some(BootStage::PtyStarting);
    tab.agent_booting = true;
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("fixture-provider"),
    ));
    assert!(fixture.service.failed_launch_restart_snapshot(&id).is_err());
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&id)
            .unwrap()
            .agent_session_id
            .as_deref(),
        Some("missing-session")
    );
}

#[test]
fn failed_launch_retains_provider_for_relaunch_after_identity_cleanup() {
    for provider in ["fixture-plain", "fixture-structured", "fixture-tool-less"] {
        let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some(provider.into());
        tab.boot_stage = Some(BootStage::SpawnFailed);
        assert_eq!(failed_provider(tab), Some(provider));
        assert!(crate::service::tab::clear_agent_identity_for_lifecycle_exit(tab));
        assert!(tab.agent_type.is_none());
        assert_eq!(failed_provider(tab), Some(provider));
        tab.agent_booting = true;
        assert_eq!(failed_provider(tab), None);
    }
}

#[test]
fn ordinary_exited_shell_is_not_a_failed_launch() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.boot_stage = Some(BootStage::ShellReady);
    assert_eq!(failed_provider(tab), None);
}

#[test]
fn failed_launch_without_a_pty_prepares_a_fresh_restart() {
    for provider in ["fixture-plain", "fixture-structured", "fixture-tool-less"] {
        let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some(provider.into());
        tab.boot_stage = Some(BootStage::SpawnFailed);
        crate::service::tab::clear_agent_identity_for_lifecycle_exit(tab);
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        fixture.set_agent_full_access(false);
        fixture.service.agent_registry.register_provider(Box::new(
            crate::agent::test_provider::MockProvider::new(provider),
        ));
        let restart = fixture
            .service
            .failed_launch_restart_snapshot(&tab_id)
            .unwrap()
            .unwrap();
        assert_eq!(restart.provider, provider);
        assert_eq!(restart.tab_id, tab_id);
        assert!(restart.pty_id.is_empty());
        assert!(restart.session_id.is_none());
    }
}
