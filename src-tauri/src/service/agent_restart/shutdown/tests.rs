use super::idle_restart_termination_allowed;

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn agent_restart_only_proven_lifecycle_exit_releases_exit_submission() {
    for proven_exit in [false, true] {
        let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
        let pty_id = format!("restart-lifecycle-exit-{proven_exit}");
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.pty_id = Some(pty_id.clone());
        tab.agent_type = Some("opaque-provider".into());
        tab.agent_session_id = Some("persisted-session".into());
        tab.session_binding_nonce = Some("owned-nonce".into());
        tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        let claim = crate::service::pty_io::submit_transaction::claim_exit_submit_cycle(&pty_id);
        let cleared = if proven_exit {
            fixture
                .service
                .clear_tab_agent_identity_after_confirmed_exit(&tab_id, "confirmed provider exit")
        } else {
            fixture
                .service
                .clear_tab_agent_identity_explicitly(&tab_id, "explicit identity clear")
        };
        assert!(cleared);
        assert_eq!(claim.provider_exited(), proven_exit);
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&tab_id).unwrap();
        assert_eq!(tab.pty_id.as_deref(), Some(pty_id.as_str()));
        assert_eq!(
            tab.prior_session_binding_nonces
                .last()
                .and_then(|binding| binding.session_id.as_deref()),
            Some("persisted-session")
        );
    }
}

#[test]
fn idle_termination_requires_the_same_authenticated_launch() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    let snap = super::TabRestartSnapshot {
        tab_id,
        pty_id: "owned-pty".into(),
        provider: "opaque-provider".into(),
        target_provider: None,
        session_id: Some("owned-session".into()),
        cwd: None,
        auto_approve: false,
        role: None,
        role_profile: None,
        handoff_history: None,
    };
    tab.pty_id = Some(snap.pty_id.clone());
    tab.agent_type = Some(snap.provider.clone());
    tab.agent_session_id = snap.session_id.clone();
    tab.session_binding_nonce = Some("owned-nonce".into());
    tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    assert!(super::verified_restart_session(&snap, tab));
    for field in 0..5 {
        let mut changed = tab.clone();
        match field {
            0 => changed.pty_id = Some("foreign-pty".into()),
            1 => changed.agent_type = Some("foreign-provider".into()),
            2 => changed.agent_session_id = Some("foreign-session".into()),
            3 => changed.reported_session_binding_nonce = None,
            _ => changed.agent_booting = true,
        }
        assert!(!super::verified_restart_session(&snap, &changed));
    }
}

#[test]
fn verified_idle_restart_terminates_directly() {
    assert!(idle_restart_termination_allowed(true, true, true, false));
}

#[test]
fn busy_restart_keeps_existing_shutdown_policy() {
    assert!(!idle_restart_termination_allowed(true, false, true, false));
}

#[test]
fn idle_restart_requires_capability_binding_and_no_background_work() {
    for evidence in [
        (false, true, true, false),
        (true, true, false, false),
        (true, true, true, true),
    ] {
        assert!(!idle_restart_termination_allowed(
            evidence.0, evidence.1, evidence.2, evidence.3
        ));
    }
}

#[test]
fn restart_path_has_no_fixed_delay_coordination() {
    for source in [
        include_str!("../shutdown.rs"),
        include_str!("../../agent_restart.rs"),
        include_str!("../../orchestrator/launch.rs"),
        include_str!("../../orchestrator/launch/owner_prompt_wait.rs"),
    ] {
        for forbidden in [
            "thread::sleep(",
            "tokio::time::sleep(",
            "setTimeout(",
            "time::interval(",
        ] {
            assert!(
                !source.contains(forbidden),
                "restart path contains {forbidden}"
            );
        }
    }
}
