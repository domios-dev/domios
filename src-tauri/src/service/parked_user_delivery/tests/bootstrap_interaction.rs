use super::*;

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires the shared interactive desktop ServiceContext fixture"
)]
fn zero_write_owner_refusal_retains_head_and_disarms_turn_confirmation() {
    let (fixture, tab) = queue_fixture(false, None);
    let first = start_current_chat_send(&fixture, &tab, FIRST_PAYLOAD, "client-a");
    let second = start_current_chat_send(&fixture, &tab, SECOND_PAYLOAD, "client-b");
    let (written_tx, written_rx) = mpsc::channel();
    let _boundary = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |record, _| {
            written_tx.send(record.send_id.clone()).unwrap();
            Err(DeliveryError::TabBusyRefused {
                tab: record.authority.tab_id.clone(),
                detail: "opaque-zero-write".into(),
            })
        }),
    );
    let (listener, settled) = settled_listener(&fixture, &tab, &first.send_id);
    mark_ready(&fixture, &tab);
    fixture.service.wake_parked_user_send(&tab);
    assert_eq!(
        written_rx.recv_timeout(TEST_DEADLINE).unwrap(),
        first.send_id
    );
    assert_eq!(
        settled.recv_timeout(TEST_DEADLINE).unwrap()["state"],
        "held"
    );
    fixture._app.unlisten(listener);
    await_owner_idle(&fixture, &tab);
    let records = fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .records(&tab)
        .unwrap();
    assert_eq!(records[0].state, UserSendState::Queued);
    assert_eq!(records[1].state, UserSendState::Queued);
    assert!(live_turn(
        &fixture,
        &tab,
        "opaque-session",
        "opaque-current-nonce",
        "unwritten-turn",
        FIRST_PAYLOAD
    )
    .is_empty());
    assert!(!fixture
        .service
        .chat_send_ledger
        .user_confirmed(&first.send_id));
    assert_reopened_originals(&fixture, &[first, second]);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires the shared interactive desktop ServiceContext fixture"
)]
fn user_admissions_leave_existing_interactive_bootstrap_record_and_bytes_unchanged() {
    let (fixture, tab) = queue_fixture(true, None);
    let digest = fixture
        .service
        .create_pending_delivery(
            &tab,
            PendingDeliveryChannel::InteractiveInput,
            "opaque-bootstrap",
        )
        .unwrap();
    let before = fixture
        .service
        .mux
        .read()
        .get_tab(&tab)
        .unwrap()
        .pending_delivery
        .clone()
        .unwrap();
    assert_eq!(before.send_id(), None);
    let first = start_current_chat_send(&fixture, &tab, FIRST_PAYLOAD, "client-a");
    let second = start_current_chat_send(&fixture, &tab, SECOND_PAYLOAD, "client-b");
    fixture.service.drain_user_owner(&tab).unwrap();
    let after = fixture
        .service
        .mux
        .read()
        .get_tab(&tab)
        .unwrap()
        .pending_delivery
        .clone()
        .unwrap();
    assert_eq!(
        serde_json::to_value(&before).unwrap(),
        serde_json::to_value(&after).unwrap()
    );
    assert_eq!(
        fixture
            .service
            .pending_delivery_payloads
            .resolve(&digest)
            .unwrap(),
        "opaque-bootstrap"
    );
    assert_reopened_originals(&fixture, &[first, second]);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires the shared interactive desktop ServiceContext fixture"
)]
fn ready_user_send_does_not_wait_for_bootstrap_submit_completion() {
    let (fixture, tab) = queue_fixture(true, None);
    let digest = fixture
        .service
        .create_pending_delivery(
            &tab,
            PendingDeliveryChannel::InteractiveInput,
            "opaque-bootstrap",
        )
        .unwrap();
    let first = start_current_chat_send(&fixture, &tab, FIRST_PAYLOAD, "client-a");
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let _bootstrap = BootstrapSubmitBoundary::install(
        &tab,
        Arc::new(move |text| {
            assert_eq!(text, "opaque-bootstrap");
            bootstrap_tx.send(()).unwrap();
            release_rx.lock().recv_timeout(TEST_DEADLINE).unwrap();
            Ok(())
        }),
    );
    let (writes_tx, writes_rx) = mpsc::channel();
    let _user = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |record, _| {
            writes_tx.send(record.send_id.clone()).unwrap();
            Err(DeliveryError::SubmitUnconfirmed {
                tab: record.authority.tab_id.clone(),
                detail: "opaque-uncertainty".into(),
            })
        }),
    );
    fixture.service.wake_parked_user_send(&tab);
    assert!(matches!(
        bootstrap_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab)
        .unwrap()
        .pending_delivery
        .is_some());
    mark_ready(&fixture, &tab);
    fixture.service.wake_parked_user_send(&tab);
    bootstrap_rx.recv_timeout(TEST_DEADLINE).unwrap();
    fixture.service.wake_parked_user_send(&tab);
    assert_eq!(
        writes_rx.recv_timeout(TEST_DEADLINE).unwrap(),
        first.send_id
    );
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab)
        .unwrap()
        .pending_delivery
        .is_none());
    release_tx.send(()).unwrap();
    await_owner_idle(&fixture, &tab);
    assert!(matches!(
        writes_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert!(matches!(
        bootstrap_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert!(fixture
        .service
        .pending_delivery_payloads
        .resolve(&digest)
        .is_err());
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires the shared interactive desktop ServiceContext fixture"
)]
fn held_user_store_does_not_suppress_existing_ready_bootstrap_consumer() {
    let (fixture, tab) = queue_fixture(true, None);
    let digest = fixture
        .service
        .create_pending_delivery(
            &tab,
            PendingDeliveryChannel::InteractiveInput,
            "opaque-bootstrap",
        )
        .unwrap();
    mark_ready(&fixture, &tab);
    fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .tab(&tab)
        .failed
        .store(true, Ordering::Release);
    let writes = Arc::new(AtomicUsize::new(0));
    let count = writes.clone();
    let _bootstrap = BootstrapSubmitBoundary::install(
        &tab,
        Arc::new(move |text| {
            assert_eq!(text, "opaque-bootstrap");
            count.fetch_add(1, Ordering::AcqRel);
            Ok(())
        }),
    );
    let _user = UserSubmitBoundary::install(
        &tab,
        Arc::new(|_, _| panic!("held user owner must not submit")),
    );
    fixture.service.drain_user_owner(&tab).unwrap();
    assert_eq!(writes.load(Ordering::Acquire), 1);
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab)
        .unwrap()
        .pending_delivery
        .is_none());
    assert!(fixture
        .service
        .pending_delivery_payloads
        .resolve(&digest)
        .is_err());
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
fn legacy_user_slot_is_held_without_replay_adoption_or_payload_cleanup() {
    let (fixture, tab) = queue_fixture(true, None);
    let digest = fixture
        .service
        .create_pending_delivery(
            &tab,
            PendingDeliveryChannel::InteractiveInput,
            "opaque-legacy-original",
        )
        .unwrap();
    {
        let mut mux = fixture.service.mux.write();
        let pending = mux
            .get_tab_mut(&tab)
            .unwrap()
            .pending_delivery
            .take()
            .unwrap()
            .with_send_id("opaque-legacy-id".into());
        mux.get_tab_mut(&tab).unwrap().pending_delivery = Some(pending);
    }
    let before = fixture
        .service
        .mux
        .read()
        .get_tab(&tab)
        .unwrap()
        .pending_delivery
        .clone()
        .unwrap();
    let _bootstrap = BootstrapSubmitBoundary::install(
        &tab,
        Arc::new(|_| panic!("legacy user slot must remain held")),
    );
    mark_ready(&fixture, &tab);
    fixture.service.drain_user_owner(&tab).unwrap();
    let after = fixture
        .service
        .mux
        .read()
        .get_tab(&tab)
        .unwrap()
        .pending_delivery
        .clone()
        .unwrap();
    assert_eq!(
        serde_json::to_value(&before).unwrap(),
        serde_json::to_value(&after).unwrap()
    );
    assert_eq!(
        fixture
            .service
            .pending_delivery_payloads
            .resolve(&digest)
            .unwrap(),
        "opaque-legacy-original"
    );
    assert!(fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .records(&tab)
        .unwrap()
        .is_empty());
}
