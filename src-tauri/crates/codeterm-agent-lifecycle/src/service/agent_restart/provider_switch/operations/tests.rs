use super::*;
use crate::service::test_fixture::ServiceFixture;

#[test]
fn self_switch_is_owned_by_daemon_after_callers_identity_is_retired() {
    for general in [false, true] {
        let (mut mux, mut id) = crate::multiplexer::state::MultiplexerState::new();
        if general {
            crate::service::general_agent::ensure_general_workspace(&mut mux, false);
            id = mux.resolve_tab_id("general").unwrap();
        }
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.agent_type = Some("source-token".into());
        tab.session_binding_nonce = Some("nonce-token".into());
        let fixture = ServiceFixture::new(mux);
        let handoffs = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let delivered = handoffs.clone();
        let accepted = fixture
            .service
            .accept_provider_switch_with(
                &id,
                "target-token".into(),
                Some(&id),
                Some("nonce-token"),
                move |svc, tab, target, own| {
                    assert!(own);
                    assert_eq!(delivered.fetch_add(1, std::sync::atomic::Ordering::SeqCst), 0);
                    let mut mux = svc.mux.write();
                    let current = mux.get_tab_mut(tab).unwrap();
                    current.agent_type = Some(target.into());
                    current.agent_session_id = Some("new-token".into());
                    current.session_binding_nonce = Some("target-nonce".into());
                    current.reported_session_binding_nonce = Some("target-nonce".into());
                    Ok(ProviderSwitchOutcome {
                        tab_id: tab.into(),
                        provider: target.into(),
                        session_id: "new-token".into(),
                        previous_provider: "source-token".into(),
                        previous_session_id: Some("old-token".into()),
                        previous_history_path: "archive-token".into(),
                        binding_restored: true,
                        handoff_delivered: true,
                    })
                },
            )
            .unwrap();
        let operation = accepted.status.operation_id.clone();
        assert_eq!(
            fixture
                .service
                .provider_switch_status(&id, &operation, false)
                .unwrap()
                .phase,
            "pending"
        );
        assert_eq!(
            fixture
                .service
                .mux
                .read()
                .get_tab(&id)
                .unwrap()
                .session_binding_nonce
                .as_deref(),
            Some("nonce-token")
        );
        accepted.response_complete.send(()).unwrap();
        let status = fixture
            .service
            .provider_switch_status(&id, &operation, true)
            .unwrap();
        assert_eq!(status.phase, "completed");
        assert!(status.outcome.unwrap().handoff_delivered);
        assert_eq!(fixture.service.provider_switch_status(&id, &operation, true).unwrap().phase, "completed");
        assert_eq!(handoffs.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(fixture
            .service
            .mux
            .read()
            .get_tab(&id)
            .unwrap()
            .session_binding_nonce
            .as_deref() == Some("target-nonce"));
    }
}

#[test]
fn stale_self_identity_refuses_and_a_disconnected_response_does_not_cancel_accepted_work() {
    let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
    mux.get_tab_mut(&id).unwrap().session_binding_nonce = Some("own-token".into());
    let fixture = ServiceFixture::new(mux);
    assert!(fixture
        .service
        .accept_provider_switch(
            &id,
            ProviderSwitchRequest {
                provider: "target".into(),
                model: None,
                reasoning_effort: None,
                force: false
            },
            Some(&id),
            Some("stale-token")
        )
        .is_err());
    let accepted = fixture
        .service
        .accept_provider_switch_with(
            &id,
            "target".into(),
            Some(&id),
            Some("own-token"),
            |_, _, _, own| {
                assert!(own);
                Err("opaque-runtime-refusal".into())
            },
        )
        .unwrap();
    let operation = accepted.status.operation_id.clone();
    drop(accepted);
    let status = fixture
        .service
        .provider_switch_status(&id, &operation, true)
        .unwrap();
    assert_eq!(status.phase, "failed");
    assert_eq!(status.error.as_deref(), Some("opaque-runtime-refusal"));
    assert!(fixture
        .service
        .provider_switch_notice(&id)
        .unwrap()
        .content
        .contains("opaque-runtime-refusal"));
}

#[test]
fn changed_team_before_accepted_work_starts_refuses_without_running_handoff() {
    let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
    mux.get_tab_mut(&id).unwrap().team_id = Some("accepted-team".into());
    let fixture = ServiceFixture::new(mux);
    let accepted = fixture.service.accept_provider_switch_with(&id, "target".into(), None, None,
        |_, _, _, _| panic!("changed authority cannot execute the accepted handoff")).unwrap();
    fixture.service.mux.write().get_tab_mut(&id).unwrap().team_id = Some("replacement-team".into());
    let operation = accepted.status.operation_id.clone();
    accepted.response_complete.send(()).unwrap();
    let status = fixture.service.provider_switch_status(&id, &operation, true).unwrap();
    assert_eq!(status.phase, "failed");
    assert!(status.error.unwrap().starts_with("provider_switch_superseded:"));
    assert_eq!(fixture.service.mux.read().get_tab(&id).unwrap().team_id.as_deref(), Some("replacement-team"));
}

#[test]
fn in_place_switch_cannot_adopt_reparented_source_after_operation_preflight() {
    for changed in ["team", "parent", "report", "depth", "account", "namespace", "nonce"] {
        let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.agent_type = Some("source".into());
        tab.agent_session_id = Some("source-session".into());
        tab.session_binding_nonce = Some("source-nonce".into());
        tab.reported_session_binding_nonce = Some("source-nonce".into());
        let fixture = ServiceFixture::new(mux);
        let accepted = fixture.service.accept_provider_switch_with(&id, "target".into(), None, None,
            move |svc, tab, target, own| {
                {
                    let mut mux = svc.mux.write();
                    let current = mux.get_tab_mut(tab).unwrap();
                    match changed {
                        "team" => current.team_id = Some("replacement-team".into()),
                        "parent" => current.parent_tab_id = Some("replacement-parent".into()),
                        "report" => current.report_target_tab_id = Some("replacement-report".into()),
                        "depth" => current.depth = current.depth.saturating_add(1),
                        "account" => current.restorable_launch.account_id = Some("replacement-account".into()),
                        "namespace" => current.restorable_launch.account_namespace = Some("replacement-namespace".into()),
                        "nonce" => current.session_binding_nonce = Some("replacement-nonce".into()),
                        _ => unreachable!(),
                    }
                }
                let before = serde_json::to_value(svc.mux.read().get_tab(tab).unwrap()).unwrap();
                let result = svc.switch_provider_in_place(tab, target, None, None, own);
                assert!(result.as_ref().unwrap_err().starts_with("provider_switch_superseded:"), "{changed}");
                assert_eq!(serde_json::to_value(svc.mux.read().get_tab(tab).unwrap()).unwrap(), before);
                result
            }).unwrap();
        let operation = accepted.status.operation_id.clone();
        accepted.response_complete.send(()).unwrap();
        let status = fixture.service.provider_switch_status(&id, &operation, true).unwrap();
        assert_eq!(status.phase, "failed");
        assert!(status.outcome.is_none());
        assert!(fixture.service.mux.read().get_tab(&id).unwrap().pty_id.is_none());
    }
}

#[test]
fn slow_switch_preflight_leaves_runtime_and_mux_responsive() {
    use std::{sync::mpsc, time::Duration};
    let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
    mux.get_tab_mut(&id).unwrap().agent_type = Some("source".into());
    let fixture = ServiceFixture::new(mux);
    let (entered, started) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let accepted = fixture
        .service
        .accept_provider_switch_with(&id, "target".into(), None, None, move |_, _, _, _| {
            entered.send(()).unwrap();
            resume
                .recv_timeout(Duration::from_secs(5))
                .map_err(|_| "fixture preflight was not released".to_string())?;
            Err("provider_switch_auth_timeout: fixture".into())
        })
        .unwrap();
    let operation = accepted.status.operation_id.clone();
    accepted.response_complete.send(()).unwrap();
    let entered_result = started.recv_timeout(Duration::from_secs(5));
    let entry = fixture.service.provider_switch_operations().0.lock()[0].1.clone();
    let expired = wait_status(&entry, true, Instant::now());
    assert_eq!(expired.phase, "running");
    assert_eq!(expired.stage.as_deref(), Some("preflight"));
    assert!(expired.error.is_none());
    assert!(fixture.service.accept_provider_switch_with(&id, "another".into(), None, None,
        |_, _, _, _| panic!("expired waiter must not admit a second handoff")).is_err());
    let (sent, received) = mpsc::channel();
    fixture.service.rt.spawn(async move {
        sent.send(()).unwrap();
    });
    let runtime_result = received.recv_timeout(Duration::from_secs(5));
    let current = fixture
        .service
        .mux
        .read()
        .get_tab(&id)
        .unwrap()
        .agent_type
        .clone();
    release.send(()).unwrap();
    let status = fixture
        .service
        .provider_switch_status(&id, &operation, true)
        .unwrap();
    entered_result.unwrap();
    runtime_result.unwrap();
    assert_eq!(current.as_deref(), Some("source"));
    assert_eq!(status.phase, "failed");
    assert!(status
        .error
        .unwrap()
        .starts_with("provider_switch_auth_timeout:"));
}

#[test]
fn exact_late_binding_reconciles_uncertainty_without_handoff_or_stale_completion() {
    let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("target".into());
    tab.session_binding_nonce = Some("current-nonce".into());
    tab.boot_stage = Some(codeterm_types::pty::BootStage::LaunchWritten);
    let fixture = ServiceFixture::new(mux);
    let target = TargetBinding::capture(&fixture.service, &id, "target", "current-nonce").unwrap();
    let operation = Arc::new(Operation {
        source: fixture.service.mux.read().get_tab(&id).unwrap().clone(),
        authorized_binding: Mutex::new(None),
        state: Mutex::new(ProviderSwitchStatus {
            operation_id: "owned-operation".into(), tab_id: id.clone(), provider: "target".into(),
            phase: "running".into(), stage: Some("target_binding".into()), outcome: None, error: None,
        }),
        changed: Condvar::new(), target: Mutex::new(Some(target.clone())),
    });
    let uncertain = await_target_binding(&fixture.service, &id, &target, Duration::ZERO).unwrap_err();
    assert!(uncertain.starts_with("provider_switch_readiness_unconfirmed:"));
    complete(&fixture.service, &operation, Err(uncertain));
    {
        let _scope = OperationScope(CURRENT.with(|current| current.replace(Some(operation.clone()))));
        progress(&fixture.service, &id, "source_retirement");
        publish_phase(&fixture.service, &operation, "running");
    }
    assert_eq!(operation.state.lock().phase, "failed");
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.agent_session_id = Some("late-session".into());
        tab.reported_session_binding_nonce = Some("current-nonce".into());
        tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);
    }
    crate::service::boot_wait::notify_boot_changed();
    assert_eq!(await_target_binding(&fixture.service, &id, &target, Duration::ZERO).unwrap(), "late-session");
    let before = operation.state.lock().error.clone();
    fixture.service.mux.write().get_tab_mut(&id).unwrap().team_id = Some("foreign-team".into());
    reconcile_late_binding(&fixture.service, &operation);
    assert_eq!(operation.state.lock().error, before);
    fixture.service.mux.write().get_tab_mut(&id).unwrap().team_id = None;
    reconcile_late_binding(&fixture.service, &operation);
    assert_eq!(operation.state.lock().stage.as_deref(), Some("target_ready"));
    assert_eq!(operation.state.lock().phase, "failed");
    assert!(operation.state.lock().outcome.is_none());
    let late_error = operation.state.lock().error.clone();
    reconcile_late_binding(&fixture.service, &operation);
    assert_eq!(operation.state.lock().error, late_error);
    fixture.service.mux.write().get_tab_mut(&id).unwrap().session_binding_nonce = Some("replacement-nonce".into());
    assert!(await_target_binding(&fixture.service, &id, &target, Duration::ZERO).unwrap_err().starts_with("provider_switch_superseded:"));
    complete(&fixture.service, &operation, Err("stale cancellation".into()));
    assert_eq!(operation.state.lock().error, late_error);
    assert_eq!(fixture.service.mux.read().get_tab(&id).unwrap().boot_stage, Some(codeterm_types::pty::BootStage::AgentReady));
}

#[tokio::test]
async fn completion_analytics_is_once_only_current_and_consent_gated() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("target-fixture".into());
    tab.agent_session_id = Some("new-session".into());
    tab.session_binding_nonce = Some("target-nonce".into());
    tab.reported_session_binding_nonce = Some("target-nonce".into());
    let fixture = ServiceFixture::new(mux);
    let recording = crate::test_support::record_product_analytics(
        &fixture.service,
        fixture.temp.path().join("analytics.json"),
    )
    .await;
    let operation = || Operation {
        source: fixture.service.mux.read().get_tab(&tab_id).unwrap().clone(),
        authorized_binding: Mutex::new(None),
        state: Mutex::new(ProviderSwitchStatus {
            operation_id: "private-operation".into(),
            tab_id: tab_id.clone(),
            provider: "target-fixture".into(),
            phase: "pending".into(),
            stage: None,
            outcome: None,
            error: None,
        }),
        changed: Condvar::new(),
        target: Mutex::new(None),
    };
    let outcome = ProviderSwitchOutcome {
        tab_id: tab_id.clone(),
        provider: "target-fixture".into(),
        session_id: "new-session".into(),
        previous_provider: "source-fixture".into(),
        previous_session_id: Some("old-session".into()),
        previous_history_path: "private-path".into(),
        binding_restored: true,
        handoff_delivered: true,
    };
    let first = operation();
    complete(&fixture.service, &first, Ok(outcome.clone()));
    complete(&fixture.service, &first, Ok(outcome.clone()));
    complete(&fixture.service, &first, Err("late cancellation".into()));
    publish_phase(&fixture.service, &first, "running");
    assert_eq!(first.state.lock().phase, "completed");
    assert!(first.state.lock().error.is_none());
    assert_eq!(
        recording.events().await,
        vec![serde_json::json!({
            "event_type":"agent_provider_changed","event_properties":{"provider_id":"other","outcome":"applied"}
        })]
    );
    complete(
        &fixture.service,
        &operation(),
        Err("private refusal".into()),
    );
    let mut stale = outcome.clone();
    stale.session_id = "stale".into();
    complete(&fixture.service, &operation(), Ok(stale));
    assert_eq!(recording.events().await.len(), 1);
    recording.set_enabled(false).await;
    complete(&fixture.service, &operation(), Ok(outcome));
    assert_eq!(recording.events().await.len(), 1);
}
