use super::*;

#[test]
fn direct_provider_death_routes_one_recovery_and_other_pty_deaths_route_none() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.pty_id = Some("direct-pty".into());
    tab.agent_type = Some("fixture-provider".into());
    tab.agent_session_id = Some("own-session".into());
    tab.session_binding_nonce = Some("own-nonce".into());
    tab.reported_session_binding_nonce = Some("own-nonce".into());
    tab.boot_stage = Some(BootStage::AgentReady);
    let attempt = crate::service::launch_readiness::LaunchAttempt {
        tab_id,
        pty_id: "direct-pty".into(),
        binding_nonce: "own-nonce".into(),
    };
    let mut launches = std::collections::HashMap::from([(attempt.pty_id.clone(), attempt.clone())]);
    let mut recoveries = 0;
    for _ in 0..2 {
        if let Some(action) =
            take_direct_recovery(&mut launches, "direct-pty", Some(tab), true, false)
        {
            assert_eq!(action, attempt);
            recoveries += 1;
        }
    }
    assert_eq!(recoveries, 1);
    for provider in [None, Some("fixture-provider")] {
        tab.agent_type = provider.map(str::to_owned);
        assert!(
            take_direct_recovery(&mut launches, "ordinary-pty", Some(tab), true, false).is_none()
        );
    }
    tab.agent_type = Some("fixture-provider".into());
    launches.insert(attempt.pty_id.clone(), attempt.clone());
    assert!(take_direct_recovery(&mut launches, "direct-pty", Some(tab), false, false).is_none());
    for (nonce, stage, shutdown) in [
        ("new-nonce", BootStage::AgentReady, false),
        ("own-nonce", BootStage::ReaderReady, false),
        ("own-nonce", BootStage::AgentReady, true),
    ] {
        tab.session_binding_nonce = Some(nonce.into());
        tab.boot_stage = Some(stage);
        launches.insert(attempt.pty_id.clone(), attempt.clone());
        assert!(
            take_direct_recovery(&mut launches, "direct-pty", Some(tab), true, shutdown).is_none()
        );
        assert!(launches.is_empty());
    }
}

#[test]
fn direct_launch_arms_exact_identity_before_fake_spawn_and_completes_on_reader_event() {
    use crate::service::launch_diagnostics::AgentLaunchOrigin;
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    mux.get_tab_mut(&tab_id).unwrap().agent_type = Some("fixture-provider".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let svc = &fixture.service;
    mark_tab_agent_launch_started(&svc.mux, &tab_id);
    let claim = svc.pty_ownership.acquire(&tab_id);
    let nonce = generation::reserve_binding_nonce(svc, &tab_id, None);
    let mut launch = LaunchGeneration::begin(&tab_id, "pending".into());
    assert!(launch.emit(svc, BootStage::PtyStarting));
    let (pty_id, registration) = arm_direct_launch(
        svc,
        &mut launch,
        &claim,
        None,
        None,
        None,
        AgentLaunchOrigin::VisibleSpawn,
        None,
        Some(nonce.clone()),
    )
    .unwrap();
    let attempt = launch.attempt().unwrap();
    assert_eq!(attempt.pty_id, pty_id);
    assert_eq!(attempt.binding_nonce, nonce);
    assert!(crate::service::launch_readiness::attempt_is_armed(&attempt));
    assert!(svc.pty.lock().get_session(&pty_id).is_none());
    let mut delivery = crate::service::launch_delivery::LaunchDeliveryGate::new();
    assert!(launch.emit(svc, BootStage::PtySpawned));
    assert!(launch.emit(svc, BootStage::ReaderReady));
    assert!(mark_direct_reader_ready(&mut launch, svc, &mut delivery));
    assert_eq!(
        svc.mux.read().get_tab(&tab_id).unwrap().boot_stage,
        Some(BootStage::LaunchWritten)
    );
    let stages: Vec<_> = fixture
        .ui
        .0
        .lock()
        .iter()
        .filter(|(name, _)| name == "tab-boot-stage")
        .map(|(_, payload)| payload["stage"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        stages,
        [
            "pty_starting",
            "pty_spawned",
            "reader_ready",
            "shell_ready",
            "agent_launching",
            "launch_written"
        ]
    );
    drop(registration);
    assert!(!crate::service::launch_readiness::attempt_is_armed(
        &attempt
    ));
    LAUNCH_REGISTRY.lock().finish(&launch);
}
