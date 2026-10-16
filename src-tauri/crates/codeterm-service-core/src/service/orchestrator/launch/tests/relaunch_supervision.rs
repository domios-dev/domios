use super::super::LAUNCH_REGISTRY;
use super::LaunchGeneration;
use crate::multiplexer::state::MultiplexerState;
use crate::service::launch_diagnostics::AgentLaunchOrigin;

#[test]
fn cancelled_readiness_registration_retires_only_its_own_attempt() {
    use crate::service::launch_readiness::{
        arm_with_origin, attempt_is_armed, retire_attempt, LaunchAttempt,
    };
    let first = LaunchAttempt {
        tab_id: "cancel-readiness-registration".into(),
        pty_id: "owned-pty".into(),
        binding_nonce: "first-nonce".into(),
    };
    arm_with_origin(first.clone(), AgentLaunchOrigin::Resume);
    let registration = super::super::generation::ReadinessRegistration::new(first.clone());
    let second = LaunchAttempt {
        binding_nonce: "second-nonce".into(),
        ..first.clone()
    };
    arm_with_origin(second.clone(), AgentLaunchOrigin::Resume);
    drop(registration);
    assert!(attempt_is_armed(&second));
    retire_attempt(&second);
    arm_with_origin(first.clone(), AgentLaunchOrigin::Resume);
    drop(super::super::generation::ReadinessRegistration::new(
        first.clone(),
    ));
    assert!(!attempt_is_armed(&first));
    arm_with_origin(first.clone(), AgentLaunchOrigin::Resume);
    let mut handed_off = super::super::generation::ReadinessRegistration::new(first.clone());
    handed_off.handoff();
    drop(handed_off);
    assert!(attempt_is_armed(&first));
    retire_attempt(&first);
}

#[test]
fn early_send_nonce_survives_initial_launch_registration() {
    for session in [None, Some("saved-session")] {
        let (mut mux, tab_id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("opaque-provider".into());
        tab.agent_session_id = session.map(str::to_string);
        tab.restored_from_snapshot = session.is_some();
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        super::super::mark_tab_agent_launch_started(&fixture.service.mux, &tab_id);
        let nonce =
            super::super::generation::reserve_binding_nonce(&fixture.service, &tab_id, session);
        let mut payload = crate::service::messenger::AgentDeliveryPayload {
            trace_id: None,
            transport: crate::service::messenger::QueuedAgentPayload::Text {
                text: "opaque-payload".into(),
            },
            observer: None,
            idempotency_key: None,
            expected_pty_id: None,
            expected_session_id: None,
            expected_binding_nonce: None,
        };
        let claim = fixture.service.pty_ownership.acquire(&tab_id);
        fixture
            .service
            .pin_agent_delivery_binding(&claim, &mut payload)
            .unwrap();
        assert!(payload.expected_pty_id.is_none());
        fixture
            .service
            .mux
            .write()
            .get_tab_mut(&tab_id)
            .unwrap()
            .pty_id = Some("registered-pty".into());
        let mut launch = LaunchGeneration::begin(&tab_id, "registered-pty".into());
        launch
            .capture_binding_nonce(
                &fixture.service,
                None,
                None,
                AgentLaunchOrigin::Resume,
                session,
                Some(nonce.clone()),
            )
            .unwrap();
        assert_eq!(launch.binding_nonce.as_deref(), Some(nonce.as_str()));
        let attempt = launch.attempt().unwrap();
        assert!(crate::service::launch_readiness::attempt_is_armed(&attempt));
        let witnesses = crate::service::process_tree::ProviderTreeWitnesses::default();
        let key = crate::service::process_tree::ProviderTreeKey {
            tab_id: tab_id.clone(),
            pty_id: attempt.pty_id.clone(),
            binding_nonce: Some(nonce.clone()),
            executable_name: "opaque-provider".into(),
        };
        witnesses.record_for_test(
            &key,
            crate::service::process_tree::ProviderProcessTreeSnapshot::Absent,
        );
        let absent = witnesses.record_for_test(
            &key,
            crate::service::process_tree::ProviderProcessTreeSnapshot::Absent,
        );
        let proof = crate::service::launch_readiness::LaunchExitProof::from_finished_marker(
            &attempt,
            &absent,
            Some(0),
            false,
            Some(false),
        )
        .unwrap();
        for _ in 0..2 {
            assert!(crate::service::launch_readiness::suspect_exit(
                &tab_id,
                Some(&attempt.pty_id),
                Some(0),
                &proof,
            ));
        }
        assert!(crate::service::launch_readiness::note_acknowledged(
            &tab_id,
            "registered-pty",
            &nonce,
        ));
        assert!(!crate::service::launch_readiness::suspect_exit(
            &tab_id,
            Some(&attempt.pty_id),
            Some(0),
            &proof,
        ));
        assert!(fixture
            .service
            .agent_delivery_binding_matches(&tab_id, &payload));
        if session.is_none() {
            assert!(launch.emit_first_turn_input_ready(&fixture.service));
            let mux = fixture.service.mux.read();
            let tab = mux.get_tab(&tab_id).unwrap();
            assert_eq!(
                tab.boot_stage,
                Some(crate::service::boot_stage::BootStage::AgentReady)
            );
            assert_eq!(tab.session_binding_nonce.as_deref(), Some(nonce.as_str()));
            assert!(tab.agent_session_id.is_none());
        }
        assert!(LAUNCH_REGISTRY.lock().finish(&launch));
        crate::service::launch_readiness::retire_attempt(&attempt);
    }
}

#[test]
fn launch_whose_reservation_was_reminted_registers_a_fresh_nonce() {
    let (mut mux, tab_id) = MultiplexerState::new();
    mux.get_tab_mut(&tab_id).unwrap().agent_type = Some("opaque-provider".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    super::super::mark_tab_agent_launch_started(&fixture.service.mux, &tab_id);
    let reserved = super::super::generation::reserve_binding_nonce(&fixture.service, &tab_id, None);
    fixture.service.rotate_session_binding_nonce(&tab_id);
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .pty_id = Some("registered-pty".into());
    let mut launch = LaunchGeneration::begin(&tab_id, "registered-pty".into());
    launch
        .capture_binding_nonce(
            &fixture.service,
            None,
            None,
            AgentLaunchOrigin::AtomicSpawn,
            None,
            Some(reserved.clone()),
        )
        .expect("a re-minted reservation must not fail the launch");
    let registered = launch.binding_nonce.clone().unwrap();
    assert_ne!(registered, reserved);
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .and_then(|tab| tab.session_binding_nonce.clone()),
        Some(registered)
    );
    assert!(LAUNCH_REGISTRY.lock().finish(&launch));
}

#[test]
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
            .capture_binding_nonce(&harness.service, None, None, origin, None, None)
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
            None,
            None,
        )
        .is_err());
}

#[test]
fn resume_launch_keeps_its_target_session_after_identity_clear() {
    assert_resumed_launch_target(AgentLaunchOrigin::Resume);
}

#[test]
fn restored_launch_reauthenticates_its_saved_history_marker() {
    assert_resumed_launch_target(AgentLaunchOrigin::RestoredSnapshot);
}

fn assert_resumed_launch_target(origin: AgentLaunchOrigin) {
    use crate::multiplexer::types::PriorBindingNonce;

    let (mut mux, tab_id) = MultiplexerState::new();
    let pty_id = "resume-target-pty";
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.pty_id = Some(pty_id.into());
        tab.prior_session_binding_nonces.push(PriorBindingNonce {
            session_id: Some("resume-target-session".into()),
            resume_target_session_id: None,
            nonce: "prior-binding-nonce".into(),
        });
    }
    let harness = crate::service::test_fixture::ServiceFixture::new(mux);
    let mut launch = LaunchGeneration::begin(&tab_id, pty_id.into());
    launch
        .capture_binding_nonce(
            &harness.service,
            None,
            None,
            origin,
            Some("resume-target-session"),
            None,
        )
        .expect("resume launch records its exact target");
    assert_eq!(
        crate::service::launch_session_detection::registered_launch_marker(&tab_id).as_deref(),
        Some("resume-target-session")
    );
    {
        let mut mux = harness.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        assert!(crate::service::tab::clear_agent_identity_for_lifecycle_exit(tab));
        assert_eq!(
            tab.prior_session_binding_nonces
                .last()
                .unwrap()
                .resume_target_session_id
                .as_deref(),
            Some("resume-target-session")
        );
    }
    LAUNCH_REGISTRY.lock().finish(&launch);
}
