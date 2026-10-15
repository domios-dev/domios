#[test]
fn resume_binding_on_first_turn_accepts_input_at_provider_ready() {
    use crate::agent::test_provider::MockProvider;

    let (mut mux, tab_id) = MultiplexerState::new();
    let pty_id = "resume-first-turn-pty";
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("resume-first-turn-fixture".into());
        tab.agent_session_id = Some("saved-session".into());
        tab.session_binding_nonce = Some("resume-first-turn-nonce".into());
        tab.pty_id = Some(pty_id.into());
        tab.restored_from_snapshot = true;
    }
    let harness = crate::service::test_fixture::ServiceFixture::new(mux);
    harness.service.agent_registry.register_provider(Box::new(
        MockProvider::new("resume-first-turn-fixture")
            .with_session_starts_on_first_user_turn()
            .with_provider_readiness_strategy(
                crate::agent::provider::ProviderReadinessStrategy::StructuralComposer {
                    timeout: std::time::Duration::from_secs(30),
                },
            ),
    ));
    assert!(harness
        .service
        .agent_registry
        .with_provider("resume-first-turn-fixture", |provider| {
            provider.provider_readiness_strategy().requires_composer()
        })
        .unwrap());

    assert!(launch_waits_for_first_turn_binding(
        &harness.service,
        &tab_id,
        "resume-first-turn-fixture",
        false,
        crate::service::launch_diagnostics::AgentLaunchOrigin::RestoredSnapshot,
    ));
    assert!(launch_waits_for_first_turn_binding(
        &harness.service,
        &tab_id,
        "resume-first-turn-fixture",
        true,
        crate::service::launch_diagnostics::AgentLaunchOrigin::RestoredSnapshot,
    ));
    assert!(!launch_waits_for_first_turn_binding(
        &harness.service,
        &tab_id,
        "resume-first-turn-fixture",
        true,
        crate::service::launch_diagnostics::AgentLaunchOrigin::VisibleSpawn,
    ));
    harness
        .service
        .agent_registry
        .register_provider(Box::new(MockProvider::new("claude-like-resume-fixture")));
    assert!(!launch_waits_for_first_turn_binding(
        &harness.service,
        &tab_id,
        "claude-like-resume-fixture",
        true,
        crate::service::launch_diagnostics::AgentLaunchOrigin::RestoredSnapshot,
    ));

    let mut launch = LaunchGeneration::begin(&tab_id, pty_id.into());
    assert!(!launch.await_provider_ack(std::time::Duration::ZERO));
    assert!(include_str!("../owner_prompt_wait.rs").contains("await_lazy_session_input_readiness"));
    let launch_source = include_str!("../../launch.rs");
    let provider_up = launch_source
        .find("owner_prompt_wait::await_input_readiness")
        .expect("first-turn readiness waits for provider composer evidence");
    let input_ready = launch_source
        .find("emit_first_turn_input_ready")
        .expect("first-turn readiness emits the input-accepting stage");
    assert!(provider_up < input_ready);
    assert!(launch.emit_first_turn_input_ready(&harness.service));
    {
        let mux = harness.service.mux.read();
        let tab = mux.get_tab(&tab_id).unwrap();
        assert_eq!(tab.boot_stage, Some(BootStage::LaunchBindingUnconfirmed));
        assert!(tab.boot_failure_reason.is_some());
        assert!(!tab.launch_session_authenticated());
    }
    harness
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .reported_session_binding_nonce = Some("resume-first-turn-nonce".into());
    assert!(launch.emit_first_turn_input_ready(&harness.service));
    let mux = harness.service.mux.read();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
    assert_eq!(tab.boot_failure_reason, None);
    assert!(tab.launch_session_authenticated());
    drop(mux);
    LAUNCH_REGISTRY.lock().finish(&launch);
}
