use super::idle_restart_termination_allowed;

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
