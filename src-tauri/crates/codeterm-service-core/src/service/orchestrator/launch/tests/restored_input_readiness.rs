#[test]
fn fresh_lazy_session_accepts_input_at_provider_ready() {
    use crate::agent::test_provider::MockProvider;

    let (mut mux, tab_id) = MultiplexerState::new();
    let pty_id = "resume-first-turn-pty";
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("resume-first-turn-fixture".into());
        tab.session_binding_nonce = Some("resume-first-turn-nonce".into());
        tab.pty_id = Some(pty_id.into());
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
        crate::service::launch_diagnostics::AgentLaunchOrigin::VisibleSpawn,
    ));
    assert!(super::user_input::requires_composer(
        &harness.service,
        "resume-first-turn-fixture",
        &tab_id,
    ));
    assert!(!launch_waits_for_first_turn_binding(
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
        .expect("first-turn readiness records that the provider accepts input");
    assert!(provider_up < input_ready);
    launch
        .capture_binding_nonce(
            &harness.service,
            None,
            None,
            crate::service::launch_diagnostics::AgentLaunchOrigin::VisibleSpawn,
            None,
            Some("resume-first-turn-nonce".into()),
        )
        .unwrap();
    assert!(launch.emit_first_turn_input_ready(&harness.service));
    {
        let mux = harness.service.mux.read();
        let tab = mux.get_tab(&tab_id).unwrap();
        assert_eq!(tab.boot_stage, Some(BootStage::ProviderReady));
        assert!(tab.boot_failure_reason.is_none());
        assert!(!tab.launch_session_authenticated());
        assert!(tab.provisional_launch.is_some());
    }
    {
        let mut mux = harness.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_session_id = Some("first-turn-session".into());
        tab.reported_session_binding_nonce = Some("resume-first-turn-nonce".into());
    }
    assert!(harness.service.retire_provisional_launch(
        &tab_id,
        &crate::service::ProviderSessionBinding {
            pty_id: pty_id.into(),
            binding_nonce: "resume-first-turn-nonce".into(),
        },
    ));
    let mux = harness.service.mux.read();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
    assert_eq!(tab.boot_failure_reason, None);
    assert!(tab.launch_session_authenticated());
    drop(mux);
    LAUNCH_REGISTRY.lock().finish(&launch);
}

#[test]
fn restored_retained_session_accepts_composer_before_binding() {
    use crate::agent::provider::ProviderReadinessStrategy;
    use crate::agent::test_provider::MockProvider;
    use codeterm_types::pty::{TabVerdict, TimelineState};

    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("resume-first-turn-fixture".into());
    tab.agent_session_id = Some("01a1507e-4cd1-7603-b5ad-2c14efde0072".into());
    tab.boot_stage = Some(BootStage::PtyStarting);
    tab.restored_from_snapshot = true;
    tab.agent_state = Some(
        TabVerdict {
            state: TimelineState::Idle,
            subtype: None,
        }
        .into(),
    );
    let harness = crate::service::test_fixture::ServiceFixture::new(mux);
    harness.service.agent_registry.register_provider(Box::new(
        MockProvider::new("resume-first-turn-fixture")
            .with_session_starts_on_first_user_turn()
            .with_provider_readiness_strategy(ProviderReadinessStrategy::StructuralComposer {
                timeout: std::time::Duration::from_secs(30),
            }),
    ));

    assert!(first_turn_binding_required(true, false));
    assert!(launch_waits_for_first_turn_binding(
        &harness.service,
        &tab_id,
        "resume-first-turn-fixture",
        false,
        crate::service::launch_diagnostics::AgentLaunchOrigin::RestoredSnapshot,
    ));
    assert!(!super::user_input::requires_composer(
        &harness.service,
        "resume-first-turn-fixture",
        &tab_id,
    ));

    harness
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .agent_session_id = None;
    assert!(super::user_input::requires_composer(
        &harness.service,
        "resume-first-turn-fixture",
        &tab_id,
    ));
}

#[test]
fn restored_lazy_restart_finishes_ready_without_synthesizing_a_binding_turn() {
    for session in [None, Some("own-resume-session")] {
        let (mut mux, tab_id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("lazy-restart-fixture".into());
        tab.agent_session_id = session.map(str::to_string);
        tab.pty_id = Some("restart-pty".into());
        tab.session_binding_nonce = Some("restart-nonce".into());
        tab.boot_stage = Some(BootStage::ProviderReady);
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        let mut launch = LaunchGeneration::begin(&tab_id, "restart-pty".into());
        launch
            .capture_binding_nonce(
                &fixture.service,
                None,
                None,
                crate::service::launch_diagnostics::AgentLaunchOrigin::RestoredSnapshot,
                None,
                Some("restart-nonce".into()),
            )
            .unwrap();
        assert!(launch.emit_first_turn_input_ready(&fixture.service));
        assert_eq!(
            super::startup_readiness::finish_waiting_for_first_turn(
                &mut launch,
                &crate::service::launch_readiness::LaunchSettlement::AwaitingFirstTurn,
                &fixture.service,
                crate::service::launch_diagnostics::AgentLaunchOrigin::RestoredSnapshot,
            ),
            Some(true)
        );
        let tab = fixture.service.mux.read().get_tab(&tab_id).unwrap().clone();
        assert_eq!(tab.boot_stage, Some(BootStage::ProviderReady));
        assert!(tab.boot_failure_reason.is_none());
        assert!(tab.pending_delivery.is_none());
        assert_eq!(tab.agent_session_id.as_deref(), session);
        assert!(crate::service::boot_stage::first_turn_input_is_ready(&tab));
    }
}
