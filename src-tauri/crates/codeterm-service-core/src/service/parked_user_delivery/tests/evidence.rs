use super::*;

#[test]
fn lazy_session_start_writes_first_send_without_a_session_id_then_classifies_exact_turn() {
    let (fixture, tab) = queue_fixture_with_session_mode(false, None, true);
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab).unwrap();
        tab.agent_session_id = None;
        tab.reported_session_binding_nonce = None;
    }
    let first = start_current_chat_send(&fixture, &tab, "opaque-lazy-first", "client-a");
    assert!(fixture.service.chat_send_anchor(&tab).anchor.is_none());
    let writes = Arc::new(AtomicUsize::new(0));
    let count = writes.clone();
    let _boundary = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |_record, _| {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
    );
    mark_ready(&fixture, &tab);
    fixture.service.wake_parked_user_send(&tab);
    await_owner_idle(&fixture, &tab);
    assert_eq!(
        writes.load(Ordering::SeqCst),
        1,
        "status={:?} owner={}",
        fixture.service.chat_send_ledger.status(&first.send_id),
        owner_state(&fixture, &tab)
    );
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&first.send_id)
            .unwrap()
            .state,
        ChatSendState::Unknown
    );
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab).unwrap();
        tab.agent_session_id = Some("opaque-session".into());
        tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    }
    append_turn(&fixture, "opaque-lazy-first");
    let delivered = live_turn(
        &fixture,
        &tab,
        "opaque-session",
        "opaque-current-nonce",
        "lazy-turn-a",
        "opaque-lazy-first",
    );
    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0].send_id, first.send_id);
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&first.send_id)
            .unwrap()
            .state,
        ChatSendState::Sent
    );
    assert_eq!(writes.load(Ordering::SeqCst), 1);
}

#[test]
fn anchorless_equal_text_requires_two_distinct_live_turn_ids() {
    let (fixture, tab) = queue_fixture(false, None);
    let first = start_current_chat_send(&fixture, &tab, "opaque-equal", "client-a");
    let second = start_current_chat_send(&fixture, &tab, "opaque-equal", "client-b");
    let (writes_tx, writes_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let write_ledger = fixture.service.chat_send_ledger.clone();
    let _boundary = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |record, _| {
            write_ledger.mark_user_written(&record.send_id);
            writes_tx.send(record.send_id.clone()).unwrap();
            release_rx.lock().recv_timeout(TEST_DEADLINE).unwrap();
            Ok(())
        }),
    );
    mark_ready(&fixture, &tab);
    fixture.service.wake_parked_user_send(&tab);
    let handoff = writes_rx.recv_timeout(TEST_DEADLINE).ok();
    assert_eq!(
        handoff.as_ref(),
        Some(&first.send_id),
        "{}",
        owner_state(&fixture, &tab)
    );
    append_turn(&fixture, "opaque-equal");
    let turn_a = observed_turn(&fixture, &tab, "turn-a", "opaque-equal");
    assert_eq!(
        deliver_turn(
            &fixture,
            &tab,
            "opaque-session",
            "opaque-current-nonce",
            &turn_a
        )[0]
        .send_id,
        first.send_id
    );
    let transcript = fixture
        .temp
        .path()
        .join("transcripts")
        .join("opaque-session.jsonl");
    std::fs::remove_file(&transcript).unwrap();
    release_tx.send(()).unwrap();
    let handoff = writes_rx.recv_timeout(TEST_DEADLINE).ok();
    assert_eq!(
        handoff.as_ref(),
        Some(&second.send_id),
        "{}",
        owner_state(&fixture, &tab)
    );
    assert!(fixture.service.chat_send_anchor(&tab).anchor.is_none());
    assert!(deliver_turn(
        &fixture,
        &tab,
        "opaque-session",
        "opaque-current-nonce",
        &turn_a
    )
    .is_empty());
    assert!(!fixture
        .service
        .chat_send_ledger
        .user_confirmed(&second.send_id));
    append_turn(&fixture, "opaque-equal");
    let turn_b = observed_turn(&fixture, &tab, "turn-b", "opaque-equal");
    assert_eq!(
        deliver_turn(
            &fixture,
            &tab,
            "opaque-session",
            "opaque-current-nonce",
            &turn_b
        )[0]
        .send_id,
        second.send_id
    );
    let (listener, settled) = settled_listener(&fixture, &tab, &second.send_id);
    release_tx.send(()).unwrap();
    assert_eq!(
        settled.recv_timeout(TEST_DEADLINE).unwrap()["send_id"],
        second.send_id
    );
    fixture.ui.unlisten(listener);
}
