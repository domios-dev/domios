use super::*;

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires the shared interactive desktop ServiceContext fixture"
)]
fn first_readiness_advances_held_user_sends_after_each_submit_unconfirmed() {
    const THIRD_PAYLOAD: &str = " opaque-c-b3e9\n\u{03bb}  ";
    let (fixture, tab) = queue_fixture(true, None);
    let first = start_current_chat_send(&fixture, &tab, FIRST_PAYLOAD, "client-a");
    let second = start_current_chat_send(&fixture, &tab, SECOND_PAYLOAD, "client-b");
    let third = start_current_chat_send(&fixture, &tab, THIRD_PAYLOAD, "client-c");
    let (writes_tx, writes_rx) = mpsc::channel();
    let service = fixture.service.clone();
    let _boundary = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |record, text| {
            service.mark_user_send_written(record);
            let written = service.chat_send_ledger.status(&record.send_id).unwrap();
            assert_eq!(written.state, ChatSendState::Written);
            assert_eq!(serde_json::to_value(&written).unwrap()["state"], "written");
            assert!(service
                .chat_send_ledger
                .set_user_pending_reason(&record.send_id, Some("late-pending-notice"))
                .is_none());
            writes_tx
                .send((record.send_id.clone(), text.to_string(), written.state))
                .unwrap();
            Ok(())
        }),
    );

    fixture.service.drain_user_owner(&tab).unwrap();
    await_owner_idle(&fixture, &tab);
    assert!(matches!(
        writes_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    let queued = fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .records(&tab)
        .unwrap();
    assert_eq!(queued.len(), 3);
    assert!(queued
        .iter()
        .all(|record| record.authority.binding_nonce == "opaque-current-nonce"));
    for accepted in [&first, &second, &third] {
        let status = fixture
            .service
            .chat_send_ledger
            .status(&accepted.send_id)
            .unwrap();
        assert_eq!(status.state, ChatSendState::Pending);
        assert!(
            status.pending_reason.is_some(),
            "status={status:?} owner={}",
            owner_state(&fixture, &tab)
        );
    }

    mark_ready(&fixture, &tab);
    fixture.service.wake_parked_user_send(&tab);
    await_owner_idle(&fixture, &tab);
    let writes = [
        writes_rx.recv_timeout(TEST_DEADLINE).unwrap(),
        writes_rx.recv_timeout(TEST_DEADLINE).unwrap(),
        writes_rx.recv_timeout(TEST_DEADLINE).unwrap(),
    ];
    assert_eq!(writes[0].0, first.send_id);
    assert_eq!(writes[1].0, second.send_id);
    assert_eq!(writes[2].0, third.send_id);
    assert_eq!(writes[0].1, FIRST_PAYLOAD.replace("\r\n", "\n"));
    assert_eq!(writes[1].1, format!("\n\n{SECOND_PAYLOAD}"));
    assert_eq!(writes[2].1, format!("\n\n{THIRD_PAYLOAD}"));
    assert!(writes
        .iter()
        .all(|(_, _, state)| state == &ChatSendState::Written));
    assert!(matches!(
        writes_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));

    for accepted in [&first, &second, &third] {
        let status = fixture
            .service
            .chat_send_ledger
            .status(&accepted.send_id)
            .unwrap();
        assert_eq!(
            status.state,
            ChatSendState::Unknown,
            "missing turn evidence leaves a written send submit_unconfirmed"
        );
        assert_eq!(
            serde_json::to_value(status).unwrap()["state"],
            "submit_unconfirmed"
        );
    }
    assert!(fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .records(&tab)
        .unwrap()
        .is_empty());

    let merged_turn = writes
        .iter()
        .map(|(_, text, _)| text.as_str())
        .collect::<String>();
    append_turn(&fixture, &merged_turn);
    let confirmations = live_turn(
        &fixture,
        &tab,
        "opaque-session",
        "opaque-current-nonce",
        "merged-held-turn",
        &merged_turn,
    );
    assert_eq!(
        confirmations
            .iter()
            .map(|status| status.send_id.as_str())
            .collect::<Vec<_>>(),
        vec![
            first.send_id.as_str(),
            second.send_id.as_str(),
            third.send_id.as_str()
        ]
    );
    assert!(confirmations
        .iter()
        .all(|status| status.state == ChatSendState::Sent));
    for accepted in [&first, &second, &third] {
        assert_eq!(
            fixture
                .service
                .chat_send_ledger
                .status(&accepted.send_id)
                .unwrap()
                .state,
            ChatSendState::Sent
        );
    }
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
fn post_ready_queued_user_sends_have_a_blank_line_and_reconcile_together() {
    let (fixture, tab) = queue_fixture(true, None);
    mark_ready(&fixture, &tab);
    let (writes_tx, writes_rx) = mpsc::channel();
    let _boundary = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |record, text| {
            writes_tx
                .send((record.send_id.clone(), text.to_string()))
                .unwrap();
            Ok(())
        }),
    );

    let first_reservation = fixture
        .service
        .reserve_user_send(&tab, Some("client-a".into()))
        .unwrap();
    let second_reservation = fixture
        .service
        .reserve_user_send(&tab, Some("client-b".into()))
        .unwrap();
    let first = fixture
        .service
        .prepare_and_admit_user_send(first_reservation, FIRST_PAYLOAD.into(), false)
        .unwrap()
        .status;
    let second = fixture
        .service
        .prepare_and_admit_user_send(second_reservation, SECOND_PAYLOAD.into(), false)
        .unwrap()
        .status;

    let (first_id, first_text) = writes_rx.recv_timeout(TEST_DEADLINE).unwrap();
    assert_eq!(first_id, first.send_id);
    let (second_id, second_text) = writes_rx.recv_timeout(TEST_DEADLINE).unwrap();
    assert_eq!(second_id, second.send_id);
    assert_eq!(first_text, FIRST_PAYLOAD.replace("\r\n", "\n"));
    assert_eq!(second_text, format!("\n\n{SECOND_PAYLOAD}"));
    assert!(matches!(
        writes_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    await_owner_idle(&fixture, &tab);
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&first.send_id)
            .unwrap()
            .state,
        ChatSendState::Unknown
    );
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&second.send_id)
            .unwrap()
            .state,
        ChatSendState::Unknown
    );

    let merged_turn = format!("{first_text}{second_text}");
    append_turn(&fixture, &merged_turn);
    let delivered = live_turn(
        &fixture,
        &tab,
        "opaque-session",
        "opaque-current-nonce",
        "merged-turn",
        &merged_turn,
    );
    assert_eq!(delivered.len(), 2);
    assert_eq!(delivered[0].send_id, first.send_id);
    assert_eq!(delivered[1].send_id, second.send_id);
    assert!(delivered
        .iter()
        .all(|status| status.state == ChatSendState::Sent));
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires the shared interactive desktop ServiceContext fixture"
)]
fn ready_user_send_writes_without_a_follow_up_output_event_while_agent_delivery_waits() {
    let (fixture, tab) = queue_fixture(true, None);
    mark_ready(&fixture, &tab);
    let agent_id = fixture
        .service
        .pending_delivery_payloads
        .agent_deliveries
        .enqueue(&tab, "pending agent delivery", None)
        .unwrap();
    let (writes_tx, writes_rx) = mpsc::channel();
    let writes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = writes.clone();
    let _boundary = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |record, text| {
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            writes_tx
                .send((record.send_id.clone(), text.to_string()))
                .unwrap();
            Ok(())
        }),
    );

    let reservation = fixture
        .service
        .reserve_user_send(&tab, Some("post-ready-idle-send".into()))
        .unwrap();
    let accepted = fixture
        .service
        .prepare_and_admit_user_send(reservation, FIRST_PAYLOAD.into(), false)
        .unwrap();
    let (send_id, payload) = writes_rx.recv_timeout(TEST_DEADLINE).unwrap();
    assert_eq!(send_id, accepted.status.send_id);
    assert_eq!(payload, FIRST_PAYLOAD.replace("\r\n", "\n"));
    await_owner_idle(&fixture, &tab);
    assert!(matches!(
        fixture.service.drain_user_owner(&tab).unwrap(),
        super::super::UserDrain::Idle
    ));

    assert_eq!(writes.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(matches!(
        writes_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert!(fixture
        .service
        .pending_delivery_payloads
        .agent_deliveries
        .has_pending(&tab)
        .unwrap());
    assert_eq!(
        fixture
            .service
            .pending_delivery_payloads
            .agent_deliveries
            .pending_head(&tab)
            .unwrap()
            .unwrap()
            .id,
        agent_id
    );
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
fn zero_write_refusal_retains_an_explicit_dispatch_wake_until_one_write() {
    let (fixture, tab) = queue_fixture(true, None);
    mark_ready(&fixture, &tab);
    let (attempts_tx, attempts_rx) = mpsc::channel();
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let attempt_count = attempts.clone();
    let writes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let write_count = writes.clone();
    let _boundary = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |record, text| {
            let attempt = attempt_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            attempts_tx.send(text.to_string()).unwrap();
            if attempt == 0 {
                Err(DeliveryError::TabBusyRefused {
                    tab: record.authority.tab_id.clone(),
                    detail: "fixture modal refusal".into(),
                })
            } else {
                write_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }
        }),
    );

    let reservation = fixture
        .service
        .reserve_user_send(&tab, Some("wake-after-refusal".into()))
        .unwrap();
    let accepted = fixture
        .service
        .prepare_and_admit_user_send(reservation, FIRST_PAYLOAD.into(), false)
        .unwrap();
    assert_eq!(
        attempts_rx.recv_timeout(TEST_DEADLINE).unwrap(),
        FIRST_PAYLOAD.replace("\r\n", "\n")
    );
    await_owner_stopped(&fixture, &tab);
    let owner = fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .tab(&tab);
    assert!(owner
        .waiting_for_dispatch
        .load(std::sync::atomic::Ordering::Acquire));

    fixture.service.wake_user_owner_on_state_change(&tab);
    assert_eq!(
        attempts_rx.recv_timeout(TEST_DEADLINE).unwrap(),
        FIRST_PAYLOAD.replace("\r\n", "\n")
    );
    await_owner_idle(&fixture, &tab);
    assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert_eq!(writes.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(matches!(
        attempts_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert!(!owner
        .waiting_for_dispatch
        .load(std::sync::atomic::Ordering::Acquire));
    assert!(fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .records(&tab)
        .unwrap()
        .is_empty());
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&accepted.status.send_id)
            .unwrap()
            .state,
        ChatSendState::Unknown
    );
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires the shared interactive desktop ServiceContext fixture"
)]
fn ready_busy_owner_writes_each_send_without_waiting_for_prior_turn_evidence() {
    let (fixture, tab) = queue_fixture(true, None);
    append_turn(&fixture, "opaque-equal");
    mark_ready(&fixture, &tab);
    fixture
        .service
        .agent_tracker
        .lock()
        .set_working("opaque-pty");
    let (writes_tx, writes_rx) = mpsc::channel();
    let _boundary = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |record, text| {
            writes_tx
                .send((record.send_id.clone(), text.to_string()))
                .unwrap();
            Ok(())
        }),
    );

    let first = fixture
        .service
        .start_chat_send(&tab, "opaque-equal".into(), None, Some("client-a".into()))
        .unwrap();
    let second = fixture
        .service
        .start_chat_send(&tab, "opaque-equal".into(), None, Some("client-b".into()))
        .unwrap();
    await_owner_idle(&fixture, &tab);
    assert_eq!(
        writes_rx.recv_timeout(TEST_DEADLINE).unwrap(),
        (first.send_id.clone(), "opaque-equal".into())
    );
    assert_eq!(
        writes_rx.recv_timeout(TEST_DEADLINE).unwrap(),
        (second.send_id.clone(), "\n\nopaque-equal".into())
    );
    assert!(matches!(
        writes_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert!(!fixture
        .service
        .chat_send_ledger
        .user_confirmed(&first.send_id));
    assert!(!fixture
        .service
        .chat_send_ledger
        .user_confirmed(&second.send_id));
    assert!(live_turn(
        &fixture,
        &tab,
        "other-session",
        "opaque-current-nonce",
        "wrong-session",
        "opaque-equal"
    )
    .is_empty());
    assert!(live_turn(
        &fixture,
        &tab,
        "opaque-session",
        "old-nonce",
        "wrong-nonce",
        "opaque-equal"
    )
    .is_empty());
    fixture.service.reconcile_chat_send_evidence(&tab);
    assert!(!fixture
        .service
        .chat_send_ledger
        .user_confirmed(&first.send_id));
    append_turn(&fixture, "opaque-equal");
    assert_eq!(
        live_turn(
            &fixture,
            &tab,
            "opaque-session",
            "opaque-current-nonce",
            "turn-a",
            "opaque-equal"
        )[0]
        .send_id,
        first.send_id
    );
    append_turn(&fixture, "opaque-equal");
    assert_eq!(
        live_turn(
            &fixture,
            &tab,
            "opaque-session",
            "opaque-current-nonce",
            "turn-b",
            "opaque-equal"
        )[0]
        .send_id,
        second.send_id
    );
    assert!(fixture
        .service
        .chat_send_ledger
        .user_confirmed(&second.send_id));
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&first.send_id)
            .unwrap()
            .state,
        ChatSendState::Sent
    );
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&second.send_id)
            .unwrap()
            .state,
        ChatSendState::Sent
    );
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires the shared interactive desktop ServiceContext fixture"
)]
fn pre_ready_uncertainty_does_not_stall_writes_or_replay_after_rebind() {
    let (fixture, tab) = queue_fixture(true, None);
    let first = start_current_chat_send(&fixture, &tab, FIRST_PAYLOAD, "client-a");
    let second = start_current_chat_send(&fixture, &tab, SECOND_PAYLOAD, "client-b");
    let writes = Arc::new(AtomicUsize::new(0));
    let count = writes.clone();
    let (writes_tx, writes_rx) = mpsc::channel();
    let service = fixture.service.clone();
    let _boundary = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |record, text| {
            count.fetch_add(1, Ordering::SeqCst);
            service.mark_user_send_written(record);
            writes_tx
                .send((record.send_id.clone(), text.to_string()))
                .unwrap();
            Err(DeliveryError::SubmitUnconfirmed {
                tab: "opaque-tab".into(),
                detail: "opaque-uncertainty".into(),
            })
        }),
    );
    let (listener, settled) = settled_listener(&fixture, &tab, &first.send_id);
    mark_ready(&fixture, &tab);
    fixture.service.wake_parked_user_send(&tab);
    assert_eq!(
        receive_resolved_status(&settled).unwrap()["state"],
        "submit_unconfirmed"
    );
    fixture._app.unlisten(listener);
    await_owner_idle(&fixture, &tab);
    let (first_write, first_text) = writes_rx.recv_timeout(TEST_DEADLINE).unwrap();
    assert_eq!(first_write, first.send_id);
    let (second_write, second_text) = writes_rx.recv_timeout(TEST_DEADLINE).unwrap();
    assert_eq!(second_write, second.send_id);
    assert_eq!(second_text, format!("\n\n{SECOND_PAYLOAD}"));
    assert!(matches!(
        writes_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert_eq!(writes.load(Ordering::SeqCst), 2);
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&second.send_id)
            .unwrap()
            .state,
        ChatSendState::Unknown
    );

    append_turn(&fixture, &first_text);
    let first_confirmation = live_turn(
        &fixture,
        &tab,
        "opaque-session",
        "opaque-current-nonce",
        "first-turn",
        &first_text,
    );
    assert_eq!(first_confirmation.len(), 1);
    assert_eq!(first_confirmation[0].send_id, first.send_id);
    append_turn(&fixture, &second_text);
    let second_confirmation = live_turn(
        &fixture,
        &tab,
        "opaque-session",
        "opaque-current-nonce",
        "second-turn",
        &second_text,
    );
    assert_eq!(second_confirmation.len(), 1);
    assert_eq!(second_confirmation[0].send_id, second.send_id);
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&first.send_id)
            .unwrap()
            .state,
        ChatSendState::Sent
    );
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&second.send_id)
            .unwrap()
            .state,
        ChatSendState::Sent
    );
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab)
        .unwrap()
        .pty_id = Some("other-pty".into());
    append_turn(&fixture, &first_text);
    fixture.service.reconcile_chat_send_evidence(&tab);
    fixture.service.drain_user_owner(&tab).unwrap();
    assert_eq!(writes.load(Ordering::SeqCst), 2);
    assert!(UserSendStore::open(fixture.temp.path())
        .unwrap()
        .records(&tab)
        .unwrap()
        .is_empty());
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires the shared interactive desktop ServiceContext fixture"
)]
fn exact_binding_after_agent_ready_releases_held_user_send_once() {
    let (fixture, tab) = queue_fixture(true, None);
    attach_fixture_pty(&fixture, &tab);
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab).unwrap();
        tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);
        tab.reported_session_binding_nonce = None;
    }
    let (writes_tx, writes_rx) = mpsc::channel();
    let service = fixture.service.clone();
    let _boundary = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |record, text| {
            service.mark_user_send_written(record);
            writes_tx.send(text.to_string()).unwrap();
            Ok(())
        }),
    );
    let accepted = start_current_chat_send(&fixture, &tab, FIRST_PAYLOAD, "binding-wake");
    fixture.service.drain_user_owner(&tab).unwrap();
    await_owner_idle(&fixture, &tab);
    assert!(matches!(
        writes_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&accepted.send_id)
            .unwrap()
            .state,
        ChatSendState::Pending
    );
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab).unwrap();
        tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    }
    fixture.service.reconcile_authenticated_agent_input(&tab);
    await_owner_idle(&fixture, &tab);
    assert_eq!(
        writes_rx.recv_timeout(TEST_DEADLINE).unwrap(),
        FIRST_PAYLOAD.replace("\r\n", "\n")
    );
    fixture.service.reconcile_authenticated_agent_input(&tab);
    await_owner_idle(&fixture, &tab);
    assert!(matches!(
        writes_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&accepted.send_id)
            .unwrap()
            .state,
        ChatSendState::Unknown
    );
}
