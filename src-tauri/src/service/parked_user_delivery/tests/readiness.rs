use super::*;

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires the shared interactive desktop ServiceContext fixture"
)]
fn restored_head_waits_with_reason_then_writes_once_after_current_nonce_binding() {
    let (fixture, tab) = queue_fixture(true, None);
    {
        let mut mux = fixture.service.mux.write();
        let restored = mux.get_tab_mut(&tab).unwrap();
        restored.boot_stage = Some(codeterm_types::pty::BootStage::LaunchWritten);
        restored.session_binding_nonce = Some("nonce-restored-current".into());
        restored.reported_session_binding_nonce = Some("nonce-before-restore".into());
        restored.agent_session_id = Some("opaque-session".into());
    }
    let accepted = admit_current_ledger(&fixture.service, &tab, FIRST_PAYLOAD, "restored-client");
    refuse_and_park_current_send(&fixture.service, accepted.clone(), FIRST_PAYLOAD);
    let writes = Arc::new(AtomicUsize::new(0));
    let count = writes.clone();
    let _submit = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |_, _| {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
    );

    fixture.service.drain_user_owner(&tab).unwrap();
    let pending = fixture
        .service
        .chat_send_ledger
        .status(&accepted.send_id)
        .unwrap();
    assert_eq!(pending.state, ChatSendState::Pending);
    assert!(pending.pending_reason.is_some());
    assert_eq!(writes.load(Ordering::SeqCst), 0);

    mark_ready(&fixture, &tab);
    fixture.service.drain_user_owner(&tab).unwrap();
    fixture.service.drain_user_owner(&tab).unwrap();

    assert_eq!(writes.load(Ordering::SeqCst), 1);
    let settled = fixture
        .service
        .chat_send_ledger
        .status(&accepted.send_id)
        .unwrap();
    assert_eq!(settled.state, ChatSendState::Unknown);
    assert_eq!(settled.pending_reason, None);
    assert!(fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .records(&tab)
        .unwrap()
        .is_empty());
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires the shared interactive desktop ServiceContext fixture"
)]
fn agent_ready_snapshot_without_its_transition_does_not_release_user_sends() {
    let (fixture, tab) = queue_fixture(true, None);
    attach_fixture_pty(&fixture, &tab);
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab).unwrap();
        tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
        tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);
    }
    let authority = fixture.service.user_send_authority(&tab).unwrap();

    assert!(!fixture.service.user_authority_is_ready(&authority));
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires the shared interactive desktop ServiceContext fixture"
)]
fn agent_ready_transition_latches_the_current_pty_generation() {
    let (fixture, tab) = queue_fixture(true, None);
    attach_fixture_pty(&fixture, &tab);
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab).unwrap();
        tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    }
    let authority = fixture.service.user_send_authority(&tab).unwrap();

    fixture
        .service
        .emit_boot_stage(&tab, codeterm_types::pty::BootStage::AgentReady, None);

    assert!(fixture.service.user_authority_is_ready(&authority));
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires the shared interactive desktop ServiceContext fixture"
)]
fn readiness_latches_for_the_current_pty_generation() {
    let (fixture, tab) = queue_fixture(true, None);
    mark_ready(&fixture, &tab);
    let authority = fixture.service.user_send_authority(&tab).unwrap();
    fixture
        .service
        .emit_boot_stage(&tab, codeterm_types::pty::BootStage::AgentReady, None);
    let mut rotated = authority.clone();
    rotated.pty_session_generation = authority
        .pty_session_generation
        .map(|generation| generation.wrapping_add(1));
    assert!(!fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .ready_launch_is_latched(&rotated));
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab)
        .unwrap()
        .boot_stage = Some(codeterm_types::pty::BootStage::SpawnFailed);
    assert!(fixture.service.user_authority_is_ready(&authority));

    let (writes_tx, writes_rx) = mpsc::channel();
    let _boundary = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |record, _| {
            writes_tx.send(record.send_id.clone()).unwrap();
            Ok(())
        }),
    );
    let reservation = fixture
        .service
        .reserve_user_send(&tab, Some("client-b".into()))
        .unwrap();
    let send_id = reservation.send_id.clone();
    let (listener, settled) = settled_listener(&fixture, &tab, &send_id);
    let accepted = fixture
        .service
        .prepare_and_admit_user_send(reservation, SECOND_PAYLOAD.into(), false)
        .unwrap()
        .status;
    let write_id = writes_rx.recv_timeout(TEST_DEADLINE).unwrap();
    assert_eq!(write_id, accepted.send_id);
    assert_eq!(
        receive_resolved_status(&settled).unwrap()["state"],
        "submit_unconfirmed"
    );
    fixture._app.unlisten(listener);
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&accepted.send_id)
            .unwrap()
            .state,
        ChatSendState::Unknown
    );
    assert!(fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .records(&tab)
        .unwrap()
        .is_empty());
}
