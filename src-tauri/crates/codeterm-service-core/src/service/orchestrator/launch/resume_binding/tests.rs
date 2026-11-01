#[test]
fn restored_unknown_store_resumes_own_target_and_hook_binds_without_user_turn() {
    use crate::multiplexer::types::PriorBindingNonce;
    let (mut state, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = state.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("lazy-fixture".into());
    tab.restored_from_snapshot = true;
    tab.session_binding_nonce = Some("new-launch".into());
    tab.prior_session_binding_nonces.push(PriorBindingNonce {
        session_id: None,
        resume_target_session_id: Some("own-resume-target".into()),
        nonce: "previous-launch".into(),
    });
    let mux = std::sync::Arc::new(codeterm_util::lock_hold::TimedRwLock::new(
        "restore-test",
        state,
    ));
    assert!(crate::service::restored_launch::restored_session_resumable(
        None
    ));
    let launch = crate::service::pty_io::claim_restored_pty_boot(&mux, &tab_id)
        .unwrap()
        .unwrap();
    assert_eq!(launch.session_id.as_deref(), Some("own-resume-target"));
    assert!(mux
        .read()
        .get_tab(&tab_id)
        .unwrap()
        .agent_session_id
        .is_none());
    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("lazy-fixture")
            .with_session_starts_on_first_user_turn(),
    ));
    let confirmation = crate::service::hook_session_binding::bind_reported_session(
        &mux,
        &registry,
        &tab_id,
        "lazy-fixture",
        "own-resume-target",
        Some("new-launch"),
    )
    .unwrap();
    assert!(confirmation.nonce_verified);
    let state = mux.read();
    let tab = state.get_tab(&tab_id).unwrap();
    assert_eq!(tab.agent_session_id.as_deref(), Some("own-resume-target"));
    assert_eq!(
        tab.reported_session_binding_nonce.as_deref(),
        Some("new-launch")
    );
    assert!(tab.pending_delivery.is_none());
}

#[test]
fn restored_target_without_current_provider_confirmation_stays_unbound() {
    let (mut state, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = state.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("lazy-fixture".into());
    tab.session_binding_nonce = Some("new-launch".into());
    let mux = std::sync::Arc::new(codeterm_util::lock_hold::TimedRwLock::new(
        "restore-refusal-test",
        state,
    ));
    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
        "lazy-fixture",
    )));
    for (session, nonce) in [("foreign", Some("foreign-launch")), ("unconfirmed", None)] {
        assert!(crate::service::hook_session_binding::bind_reported_session(
            &mux,
            &registry,
            &tab_id,
            "lazy-fixture",
            session,
            nonce,
        )
        .is_err());
        assert!(mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .agent_session_id
            .is_none());
    }
    assert!(!crate::service::restored_launch::restored_session_resumable(Some(false)));
}
