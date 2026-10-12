use super::*;

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires the shared interactive desktop ServiceContext fixture"
)]
fn fresh_prelaunch_send_waits_for_pty_and_rebinds_to_written_launch_once() {
    let (fixture, tab) = queue_fixture(true, None);
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab).unwrap();
        tab.pty_id = None;
        tab.boot_stage = Some(codeterm_types::pty::BootStage::PtyStarting);
        tab.agent_booting = true;
        tab.session_binding_nonce = Some("nonce-before-command".into());
        tab.reported_session_binding_nonce = None;
    }

    let reservation = fixture
        .service
        .reserve_user_send(&tab, Some("fresh-launch-client".into()))
        .expect("pre-PTY Chat admission is reserved against the fresh launch");
    let accepted = fixture
        .service
        .prepare_and_admit_user_send(reservation, FIRST_PAYLOAD.into(), false)
        .unwrap()
        .status;
    await_owner_idle(&fixture, &tab);

    let held = fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .head(&tab)
        .unwrap()
        .unwrap();
    assert!(held.authority.prelaunch);
    assert_eq!(held.authority.pty_id, None);
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&accepted.send_id)
            .unwrap()
            .state,
        ChatSendState::Pending
    );
    let writes = Arc::new(AtomicUsize::new(0));
    let writes_seen = writes.clone();
    let _submit = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |_, _| {
            writes_seen.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
    );

    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab).unwrap();
        tab.pty_id = Some("opaque-pty".into());
        tab.boot_stage = Some(codeterm_types::pty::BootStage::ReaderReady);
    }
    attach_fixture_pty(&fixture, &tab);
    fixture.service.drain_user_owner(&tab).unwrap();
    assert_eq!(writes.load(Ordering::SeqCst), 0);

    let pty_bound = fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .head(&tab)
        .unwrap()
        .unwrap();
    assert_eq!(pty_bound.authority.pty_id.as_deref(), Some("opaque-pty"));
    assert!(pty_bound.authority.pty_session_generation.is_some());
    assert!(pty_bound.authority.prelaunch);

    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab).unwrap();
        tab.remember_superseded_binding_nonce();
        tab.session_binding_nonce = Some("nonce-written-launch".into());
        tab.boot_stage = Some(codeterm_types::pty::BootStage::LaunchWritten);
    }
    fixture.service.drain_user_owner(&tab).unwrap();
    assert_eq!(writes.load(Ordering::SeqCst), 0);

    let rebound = fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .head(&tab)
        .unwrap()
        .unwrap();
    assert_eq!(rebound.authority.binding_nonce, "nonce-written-launch");
    assert!(!rebound.authority.prelaunch);
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&accepted.send_id)
            .unwrap()
            .state,
        ChatSendState::Pending
    );

    mark_ready(&fixture, &tab);
    fixture.service.wake_parked_user_send(&tab);
    await_owner_idle(&fixture, &tab);

    assert_eq!(writes.load(Ordering::SeqCst), 1);
    assert!(fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .records(&tab)
        .unwrap()
        .is_empty());
    let settled = fixture
        .service
        .chat_send_ledger
        .status(&accepted.send_id)
        .unwrap();
    assert!(matches!(
        settled.state.clone(),
        ChatSendState::Sent | ChatSendState::Unknown
    ));
    assert_eq!(settled.error, None);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires the shared interactive desktop ServiceContext fixture"
)]
fn queued_prelaunch_send_expires_after_a_later_launch_changes_its_session() {
    let (fixture, tab) = queue_fixture(true, None);
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab).unwrap();
        tab.pty_id = None;
        tab.boot_stage = Some(codeterm_types::pty::BootStage::PtyStarting);
        tab.agent_booting = true;
        tab.session_binding_nonce = Some("nonce-before-first-launch".into());
        tab.reported_session_binding_nonce = None;
    }
    let reservation = fixture
        .service
        .reserve_user_send(&tab, Some("expired-prelaunch-client".into()))
        .unwrap();
    let accepted = fixture
        .service
        .prepare_and_admit_user_send(reservation, FIRST_PAYLOAD.into(), false)
        .unwrap()
        .status;
    await_owner_idle(&fixture, &tab);

    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab).unwrap();
        tab.pty_id = Some("opaque-pty".into());
        tab.remember_superseded_binding_nonce();
        tab.session_binding_nonce = Some("nonce-first-launch".into());
        tab.boot_stage = Some(codeterm_types::pty::BootStage::LaunchWritten);
        tab.remember_superseded_binding_nonce();
        tab.session_binding_nonce = Some("nonce-second-launch".into());
        tab.agent_session_id = Some("different-session".into());
        tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentLaunching);
    }
    attach_fixture_pty(&fixture, &tab);
    fixture.service.drain_user_owner(&tab).unwrap();

    let expired = fixture
        .service
        .chat_send_ledger
        .status(&accepted.send_id)
        .unwrap();
    assert_eq!(expired.state, ChatSendState::Failed);
    assert_eq!(expired.error.as_deref(), Some("launch_ended_before_write"));
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
fn previous_launch_never_written_head_is_settled_before_restored_send_flushes() {
    let (fixture, tab) = queue_fixture(true, None);
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab)
        .unwrap()
        .agent_session_id = Some("previous-session".into());
    let stale = start_current_chat_send(&fixture, &tab, FIRST_PAYLOAD, "previous-launch-client");
    let stale_record = fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .head(&tab)
        .unwrap()
        .unwrap();
    assert_eq!(stale_record.authority.pty_session_generation, None);

    {
        let mut mux = fixture.service.mux.write();
        let restored = mux.get_tab_mut(&tab).unwrap();
        restored.session_binding_nonce = Some("nonce-restored-current".into());
        restored.agent_session_id = Some("opaque-session".into());
        restored.reported_session_binding_nonce = Some("nonce-restored-current".into());
        restored.boot_stage = Some(codeterm_types::pty::BootStage::LaunchWritten);
        restored.boot_failure_reason = None;
    }
    let generation = attach_fixture_pty(&fixture, &tab);
    let (writes_tx, writes_rx) = mpsc::channel();
    let _submit = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |record, _| {
            writes_tx.send(record.send_id.clone()).unwrap();
            Ok(())
        }),
    );
    let reservation = fixture
        .service
        .reserve_user_send(&tab, Some("restored-client".into()))
        .unwrap();
    let restored = fixture
        .service
        .prepare_and_admit_user_send(reservation, SECOND_PAYLOAD.into(), false)
        .unwrap()
        .status;
    await_owner_idle(&fixture, &tab);

    let expired = fixture
        .service
        .chat_send_ledger
        .status(&stale.send_id)
        .unwrap();
    assert_eq!(expired.state, ChatSendState::Failed);
    assert_eq!(expired.error.as_deref(), Some("launch_ended_before_write"));
    assert!(fixture
        .service
        .chat_send_ledger
        .reconcile(&tab)
        .contains(&expired));
    assert!(fixture
        .service
        .chat_send_ledger
        .confirm_send(&stale.send_id)
        .is_none());
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .finish(&stale.send_id, Ok(()))
            .unwrap(),
        expired
    );
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&stale.send_id)
            .unwrap(),
        expired
    );
    assert_eq!(writes_rx.try_recv(), Err(mpsc::TryRecvError::Empty));
    let queued = fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .records(&tab)
        .unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].send_id, restored.send_id);
    assert_eq!(queued[0].authority.binding_nonce, "nonce-restored-current");
    assert_eq!(queued[0].authority.pty_session_generation, Some(generation));
    assert_eq!(
        fixture
            .service
            .pending_delivery_payloads
            .user_sends
            .original(&tab, &stale.send_id)
            .unwrap_err(),
        "user send is not indexed"
    );
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&restored.send_id)
            .unwrap()
            .state,
        ChatSendState::Pending
    );
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&restored.send_id)
            .unwrap()
            .pending_reason
            .as_deref(),
        Some("exact_binding_present_stage_pending")
    );

    mark_ready(&fixture, &tab);
    fixture.service.wake_parked_user_send(&tab);
    await_owner_idle(&fixture, &tab);

    assert_eq!(
        writes_rx.recv_timeout(TEST_DEADLINE).unwrap(),
        restored.send_id
    );
    assert!(matches!(
        writes_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&stale.send_id)
            .unwrap(),
        expired
    );
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&restored.send_id)
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
