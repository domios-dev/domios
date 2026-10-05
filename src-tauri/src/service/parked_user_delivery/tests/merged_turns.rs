use super::*;

#[test]
#[cfg_attr(not(windows), ignore = "requires the shared interactive desktop ServiceContext fixture")]
fn current_launch_pre_ready_user_sends_wait_for_each_exact_turn() {
    let (fixture, tab) = queue_fixture(true, None);
    let first = start_current_chat_send(&fixture, &tab, FIRST_PAYLOAD, "client-a");
    let second = start_current_chat_send(&fixture, &tab, SECOND_PAYLOAD, "client-b");
    let (writes_tx, writes_rx) = mpsc::channel();
    let _boundary = UserSubmitBoundary::install(&tab, Arc::new(move |record, text| {
        writes_tx.send((record.send_id.clone(), text.to_string())).unwrap();
        Ok(())
    }));

    fixture.service.drain_user_owner(&tab).unwrap();
    await_owner_idle(&fixture, &tab);
    assert!(matches!(writes_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    let queued = fixture.service.pending_delivery_payloads.user_sends.records(&tab).unwrap();
    assert_eq!(queued.len(), 2);
    assert!(queued.iter().all(|record| record.authority.binding_nonce == "opaque-current-nonce"));
    for accepted in [&first, &second] {
        let status = fixture.service.chat_send_ledger.status(&accepted.send_id).unwrap();
        assert_eq!(status.state, ChatSendState::Pending);
        assert!(status.pending_reason.is_some(), "status={status:?} owner={}", owner_state(&fixture, &tab));
    }

    mark_ready(&fixture, &tab);
    fixture.service.wake_parked_user_send(&tab);
    await_owner_idle(&fixture, &tab);
    assert_eq!(
        writes_rx.recv_timeout(TEST_DEADLINE).unwrap(),
        (first.send_id.clone(), FIRST_PAYLOAD.to_string()),
    );
    assert!(matches!(writes_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    assert_eq!(fixture.service.chat_send_ledger.status(&first.send_id).unwrap().state, ChatSendState::Unknown);
    assert_eq!(fixture.service.chat_send_ledger.status(&second.send_id).unwrap().state, ChatSendState::Pending);

    append_turn(&fixture, FIRST_PAYLOAD);
    let first_confirmation = live_turn(
        &fixture,
        &tab,
        "opaque-session",
        "opaque-current-nonce",
        "first-turn",
        FIRST_PAYLOAD,
    );
    assert_eq!(first_confirmation.len(), 1);
    assert_eq!(first_confirmation[0].send_id, first.send_id);
    await_owner_idle(&fixture, &tab);

    assert_eq!(
        writes_rx.recv_timeout(TEST_DEADLINE).unwrap(),
        (second.send_id.clone(), SECOND_PAYLOAD.to_string()),
    );
    assert!(matches!(writes_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    assert_eq!(fixture.service.chat_send_ledger.status(&second.send_id).unwrap().state, ChatSendState::Unknown);

    append_turn(&fixture, SECOND_PAYLOAD);
    let second_confirmation = live_turn(
        &fixture,
        &tab,
        "opaque-session",
        "opaque-current-nonce",
        "second-turn",
        SECOND_PAYLOAD,
    );
    assert_eq!(second_confirmation.len(), 1);
    assert_eq!(second_confirmation[0].send_id, second.send_id);
    assert_eq!(fixture.service.chat_send_ledger.status(&first.send_id).unwrap().state, ChatSendState::Sent);
    assert_eq!(fixture.service.chat_send_ledger.status(&second.send_id).unwrap().state, ChatSendState::Sent);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires the shared interactive desktop ServiceContext fixture")]
fn post_ready_queued_user_sends_have_a_blank_line_and_reconcile_together() {
    let (fixture, tab) = queue_fixture(true, None);
    mark_ready(&fixture, &tab);
    let (writes_tx, writes_rx) = mpsc::channel();
    let _boundary = UserSubmitBoundary::install(&tab, Arc::new(move |record, text| {
        writes_tx.send((record.send_id.clone(), text.to_string())).unwrap();
        Ok(())
    }));

    let first = start_current_chat_send(&fixture, &tab, FIRST_PAYLOAD, "client-a");
    let second = start_current_chat_send(&fixture, &tab, SECOND_PAYLOAD, "client-b");

    assert_eq!(
        writes_rx.recv_timeout(TEST_DEADLINE).unwrap(),
        (first.send_id.clone(), FIRST_PAYLOAD.to_string()),
    );
    assert_eq!(
        writes_rx.recv_timeout(TEST_DEADLINE).unwrap(),
        (second.send_id.clone(), format!("\n\n{SECOND_PAYLOAD}")),
    );
    assert!(matches!(writes_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    assert_eq!(fixture.service.chat_send_ledger.status(&first.send_id).unwrap().state, ChatSendState::Unknown);
    assert_eq!(fixture.service.chat_send_ledger.status(&second.send_id).unwrap().state, ChatSendState::Unknown);

    let merged_turn = format!("{FIRST_PAYLOAD}\n\n{SECOND_PAYLOAD}");
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
