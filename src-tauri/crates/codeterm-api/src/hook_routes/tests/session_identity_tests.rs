use super::*;
use codeterm_service_core::service::provider_identity::AgentTypeWriteOutcome;
use codeterm_service_core::service::test_fixture::ServiceFixture;

fn service_fixture_with_sessions(
    items: Vec<(&'static str, Vec<&'static str>)>,
) -> (ServiceFixture, String) {
    let (mux, tab_id) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    register_hook_providers(fixture.service.agent_registry.as_ref(), items);
    (fixture, tab_id)
}

#[test]
fn reported_session_rejects_wrong_provider_for_locked_tab() {
    let (fixture, tab_id) = service_fixture_with_sessions(vec![
        ("claude", vec!["sid-a", "sid-b", "ses_shared"]),
        ("codex", vec!["sid-a", "sid-b", "ses_shared"]),
    ]);
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .cwd = Some("/tmp/project".into());

    let claim = bind_reported_session(
        &fixture.service.mux,
        fixture.service.agent_registry.as_ref(),
        &tab_id,
        "claude",
        "sid-a",
        None,
    )
    .unwrap();
    assert!(matches!(
        fixture.service.write_hook_tab_agent_type_from_claim(
            &tab_id,
            "claude".into(),
            Some(&tab_id),
            claim.identity_evidence(),
        ),
        Some(AgentTypeWriteOutcome::Applied { .. })
    ));

    let err = bind_reported_session(
        &fixture.service.mux,
        fixture.service.agent_registry.as_ref(),
        &tab_id,
        "codex",
        "sid-b",
        None,
    )
    .unwrap_err();

    assert_eq!(err.0, StatusCode::CONFLICT);
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .agent_session_id
            .as_deref(),
        Some("sid-a")
    );
}

#[test]
fn reported_session_binds_sid_owned_by_provider() {
    let (fixture, tab_id) = service_fixture_with_sessions(vec![("claude", vec!["claude-real"])]);
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .cwd = Some("/tmp/project".into());

    let claim = bind_reported_session(
        &fixture.service.mux,
        fixture.service.agent_registry.as_ref(),
        &tab_id,
        "claude",
        "claude-real",
        Some("nonce-1"),
    )
    .unwrap();
    assert!(matches!(
        fixture.service.write_hook_tab_agent_type_from_claim(
            &tab_id,
            "claude".into(),
            Some(&tab_id),
            claim.identity_evidence(),
        ),
        Some(AgentTypeWriteOutcome::Applied { .. })
    ));

    let g = fixture.service.mux.read();
    let tab = g.get_tab(&tab_id).unwrap();
    assert_eq!(tab.agent_type.as_deref(), Some("claude"));
    assert_eq!(tab.agent_session_id.as_deref(), Some("claude-real"));
}

#[test]
fn fresh_launch_with_matching_nonce_accepts_unpersisted_session() {
    let (fixture, tab_id) = service_fixture_with_sessions(vec![("claude", vec![])]);
    {
        let mut g = fixture.service.mux.write();
        let tab = g.get_tab_mut(&tab_id).unwrap();
        tab.cwd = Some("/tmp/project".into());
        tab.session_binding_nonce = Some("launch-nonce".into());
    }

    let claim = bind_reported_session(
        &fixture.service.mux,
        fixture.service.agent_registry.as_ref(),
        &tab_id,
        "claude",
        "fresh-unpersisted-sid",
        Some("launch-nonce"),
    )
    .expect("fresh launch with matching nonce must be accepted");
    assert!(claim.changed);
    assert!(claim.nonce_verified);
    assert!(matches!(
        fixture.service.write_hook_tab_agent_type_from_claim(
            &tab_id,
            "claude".into(),
            Some(&tab_id),
            claim.identity_evidence(),
        ),
        Some(AgentTypeWriteOutcome::Applied { .. })
    ));

    let g = fixture.service.mux.read();
    let tab = g.get_tab(&tab_id).unwrap();
    assert_eq!(tab.agent_type.as_deref(), Some("claude"));
    assert_eq!(
        tab.agent_session_id.as_deref(),
        Some("fresh-unpersisted-sid")
    );
}

/// The first-run contradiction, end to end on a real ServiceContext: a tab
/// latched at `spawn_failed` whose agent then authenticates its own launch must
/// not keep reporting a failed spawn, and no message to the tab is involved.
#[test]
fn a_nonce_verified_binding_retires_a_latched_spawn_failure() {
    let (fixture, tab_id) = service_fixture_with_sessions(vec![("claude", vec![])]);
    {
        let mut g = fixture.service.mux.write();
        let tab = g.get_tab_mut(&tab_id).unwrap();
        tab.cwd = Some("/tmp/project".into());
        tab.agent_type = Some("claude".into());
        tab.session_binding_nonce = Some("launch-nonce".into());
        tab.boot_stage = Some(codeterm_types::pty::BootStage::SpawnFailed);
        tab.boot_failure_reason = Some("submit_unconfirmed: acknowledgement not observed".into());
    }

    let claim = bind_reported_session(
        &fixture.service.mux,
        fixture.service.agent_registry.as_ref(),
        &tab_id,
        "claude",
        "late-bound-sid",
        Some("launch-nonce"),
    )
    .expect("the launch's own agent must be allowed to bind");
    assert!(claim.nonce_verified);

    fixture
        .service
        .reconcile_boot_stage_with_session_binding(&tab_id);

    let g = fixture.service.mux.read();
    let tab = g.get_tab(&tab_id).unwrap();
    assert_eq!(
        tab.boot_stage,
        Some(codeterm_types::pty::BootStage::AgentReady),
    );
    assert_eq!(tab.boot_failure_reason, None);
}

#[test]
fn unchanged_acknowledged_hook_report_still_marks_relaunch_live() {
    let (fixture, tab_id) = service_fixture_with_sessions(vec![("claude", vec!["sid-a"])]);
    let pty_id = "pty-relaunch";
    let nonce = "relaunch-nonce";
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.cwd = Some("/tmp/project".into());
        tab.agent_type = Some("claude".into());
        tab.agent_session_id = Some("sid-a".into());
        tab.pty_id = Some(pty_id.into());
        tab.session_binding_nonce = Some(nonce.into());
        tab.reported_session_binding_nonce = Some(nonce.into());
        tab.boot_stage = Some(codeterm_types::pty::BootStage::SpawnFailed);
        tab.boot_failure_reason = Some("launch deadline elapsed".into());
    }
    let attempt = codeterm_service_core::service::launch_readiness::LaunchAttempt {
        tab_id: tab_id.clone(),
        pty_id: pty_id.into(),
        binding_nonce: nonce.into(),
    };
    codeterm_service_core::service::launch_readiness::arm(attempt.clone());

    let claim = bind_reported_session(
        &fixture.service.mux,
        fixture.service.agent_registry.as_ref(),
        &tab_id,
        "claude",
        "sid-a",
        Some(nonce),
    )
    .expect("duplicate verified report should be accepted");
    assert!(claim.is_unchanged_acknowledged_report());

    let response = unchanged_acknowledged_hook_report(
        &fixture.service,
        &tab_id,
        "claude",
        "sid-a",
        "resume",
        &claim,
        Some(nonce),
    );

    assert_eq!(response.0["changed"], false);
    assert_eq!(
        codeterm_service_core::service::launch_readiness::observe(
            &attempt,
            std::time::Instant::now()
        ),
        codeterm_service_core::service::launch_readiness::LaunchObservation::Evidence(
            codeterm_service_core::service::launch_readiness::LaunchEvidence::Acknowledged,
        ),
    );
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert_eq!(
        tab.boot_stage,
        Some(codeterm_types::pty::BootStage::AgentReady),
    );
    assert_eq!(tab.boot_failure_reason, None);
}

/// The over-correction guard: a report that cannot prove the launch leaves the
/// failure exactly where it was.
#[test]
fn an_unproven_report_leaves_a_spawn_failure_latched() {
    let (fixture, tab_id) = service_fixture_with_sessions(vec![("claude", vec![])]);
    {
        let mut g = fixture.service.mux.write();
        let tab = g.get_tab_mut(&tab_id).unwrap();
        tab.cwd = Some("/tmp/project".into());
        tab.agent_type = Some("claude".into());
        tab.agent_session_id = Some("some-sid".into());
        tab.session_binding_nonce = Some("launch-nonce".into());
        tab.boot_stage = Some(codeterm_types::pty::BootStage::SpawnFailed);
        tab.boot_failure_reason = Some("PtyEof".into());
    }

    fixture
        .service
        .reconcile_boot_stage_with_session_binding(&tab_id);

    let g = fixture.service.mux.read();
    let tab = g.get_tab(&tab_id).unwrap();
    assert_eq!(
        tab.boot_stage,
        Some(codeterm_types::pty::BootStage::SpawnFailed),
    );
    assert_eq!(tab.boot_failure_reason.as_deref(), Some("PtyEof"));
}

#[test]
fn queued_user_head_releases_through_actual_current_nonce_acknowledgment() {
    use codeterm_service_core::service::parked_user_delivery::tests::{
        queue_fixture, UserSubmitBoundary,
    };
    let (fixture, tab_id) = queue_fixture(false, None);
    codeterm_service_core::service::parked_user_delivery::tests::attach_fixture_pty(
        &fixture, &tab_id,
    );
    let first = fixture
        .service
        .start_chat_send(
            &tab_id,
            "opaque-hook-first".into(),
            None,
            Some("client-a".into()),
        )
        .unwrap();
    let second = fixture
        .service
        .start_chat_send(
            &tab_id,
            "opaque-hook-second".into(),
            None,
            Some("client-b".into()),
        )
        .unwrap();
    let (write_tx, write_rx) = std::sync::mpsc::channel();
    let _boundary = UserSubmitBoundary::install(
        &tab_id,
        Arc::new(move |record, text| {
            write_tx
                .send((record.send_id.clone(), text.to_string()))
                .unwrap();
            Err(
                codeterm_service_core::service::delivery_error::DeliveryError::SubmitUnconfirmed {
                    tab: record.authority.tab_id.clone(),
                    detail: "opaque-held-uncertainty".into(),
                },
            )
        }),
    );
    fixture.service.drain_user_owner(&tab_id).unwrap();
    assert!(matches!(
        write_rx.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    let claim = bind_reported_session(
        &fixture.service.mux,
        fixture.service.agent_registry.as_ref(),
        &tab_id,
        "opaque-provider",
        "opaque-session",
        Some("opaque-current-nonce"),
    )
    .unwrap();
    assert!(claim.nonce_verified);
    let identity = acknowledge_and_reconcile_session_binding(
        &fixture.service,
        &tab_id,
        &claim,
        Some("opaque-current-nonce"),
    )
    .unwrap();
    assert_eq!(identity.pty_id, "opaque-pty");
    assert_eq!(identity.binding_nonce, "opaque-current-nonce");
    assert_eq!(
        write_rx.recv_timeout(Duration::from_secs(10)).unwrap(),
        (first.send_id.clone(), "opaque-hook-first".into())
    );
    let tab = fixture.service.mux.read().get_tab(&tab_id).unwrap().clone();
    assert_eq!(
        tab.boot_stage,
        Some(codeterm_types::pty::BootStage::AgentReady)
    );
    assert!(tab.launch_session_authenticated());
    let records = fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .records(&tab_id)
        .unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[1].send_id, second.send_id);
    assert_eq!(
        records[1].state,
        codeterm_service_core::service::pending_delivery_payloads::user_sends::UserSendState::Queued
    );
    assert!(matches!(
        write_rx.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
}

#[test]
fn stale_nonce_ack_does_not_release_and_rebound_launch_expires_queued_user_owner() {
    use codeterm_service_core::service::parked_user_delivery::tests::{
        await_owner_idle, queue_fixture, UserSubmitBoundary,
    };
    let (fixture, tab_id) = queue_fixture(false, None);
    let first = fixture
        .service
        .start_chat_send(&tab_id, "opaque-held-first".into(), None, None)
        .unwrap();
    let second = fixture
        .service
        .start_chat_send(&tab_id, "opaque-held-second".into(), None, None)
        .unwrap();
    await_owner_idle(&fixture, &tab_id);
    let writes = Arc::new(AtomicUsize::new(0));
    let count = writes.clone();
    let _boundary = UserSubmitBoundary::install(
        &tab_id,
        Arc::new(move |_, _| {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
    );
    let refused = bind_reported_session(
        &fixture.service.mux,
        fixture.service.agent_registry.as_ref(),
        &tab_id,
        "opaque-provider",
        "opaque-session",
        Some("opaque-old-nonce"),
    );
    assert!(refused.is_err());
    await_owner_idle(&fixture, &tab_id);
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    let retained = fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .records(&tab_id)
        .unwrap();
    assert_eq!(retained.len(), 2);
    assert_eq!(retained[0].send_id, first.send_id);
    assert_eq!(retained[1].send_id, second.send_id);

    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .pty_id = Some("opaque-new-pty".into());
    let claim = bind_reported_session(
        &fixture.service.mux,
        fixture.service.agent_registry.as_ref(),
        &tab_id,
        "opaque-provider",
        "opaque-rebound-session",
        Some("opaque-current-nonce"),
    )
    .unwrap();
    let identity = acknowledge_and_reconcile_session_binding(
        &fixture.service,
        &tab_id,
        &claim,
        Some("opaque-current-nonce"),
    )
    .unwrap();
    assert_eq!(identity.pty_id, "opaque-new-pty");
    await_owner_idle(&fixture, &tab_id);
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    let store = &fixture.service.pending_delivery_payloads.user_sends;
    let remaining = store.records(&tab_id).unwrap();
    assert!(
        remaining.is_empty(),
        "rebound authority {:#?} left queued records {remaining:#?}",
        fixture.service.user_send_authority(&tab_id).unwrap(),
    );
    for send_id in [&first.send_id, &second.send_id] {
        let status = fixture.service.chat_send_ledger.status(send_id).unwrap();
        assert_eq!(
            status.state,
            codeterm_service_core::service::chat_send_transaction::ChatSendState::Failed
        );
        assert_eq!(status.error.as_deref(), Some("launch_ended_before_write"));
        assert!(store.original(&tab_id, send_id).is_err());
    }
}
