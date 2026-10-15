use super::*;

#[test]
fn concurrent_drain_cannot_skip_the_admission_owners_claimed_head() {
    let (fixture, tab) = queue_fixture(true, None);
    mark_ready(&fixture, &tab);
    let (claimed_tx, claimed_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let (writes_tx, writes_rx) = mpsc::channel();
    let first_write_tx = writes_tx.clone();
    let _boundary = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |record, text| {
            claimed_tx.send(record.send_id.clone()).unwrap();
            release_rx.lock().recv_timeout(TEST_DEADLINE).unwrap();
            first_write_tx.send(text.to_owned()).unwrap();
            Ok(())
        }),
    );
    let first = fixture
        .service
        .start_chat_send(&tab, "first".into(), None, None)
        .unwrap();
    assert_eq!(
        claimed_rx.recv_timeout(TEST_DEADLINE).unwrap(),
        first.send_id
    );
    let second = fixture
        .service
        .start_chat_send(&tab, "second".into(), None, None)
        .unwrap();
    let outcome = fixture
        .service
        .drain_user_owner_with(&tab, |_, text, _, _| {
            writes_tx.send(text.to_owned()).unwrap();
            Ok(UserWrite::Finished)
        });
    let overtaking_write = writes_rx.try_recv();
    release_tx.send(()).unwrap();
    assert!(matches!(outcome.unwrap(), UserDrain::Idle));
    assert!(matches!(overtaking_write, Err(mpsc::TryRecvError::Empty)));
    assert_eq!(writes_rx.recv_timeout(TEST_DEADLINE).unwrap(), "first");
    assert_eq!(
        claimed_rx.recv_timeout(TEST_DEADLINE).unwrap(),
        second.send_id
    );
    release_tx.send(()).unwrap();
    assert_eq!(writes_rx.recv_timeout(TEST_DEADLINE).unwrap(), "\n\nsecond");
    await_owner_idle(&fixture, &tab);
    fixture.service.drain_user_owner(&tab).unwrap();
    assert!(matches!(
        writes_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert!(fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .records(&tab)
        .unwrap()
        .is_empty());
}

#[test]
fn ready_dispatch_preserves_admission_order_when_wakes_arrive_reversed() {
    let (fixture, tab) = queue_fixture(true, None);
    mark_ready(&fixture, &tab);
    let store = &fixture.service.pending_delivery_payloads.user_sends;
    let authority = fixture.service.user_send_authority(&tab).unwrap();
    let mut records = Vec::new();
    for (sequence, text) in [(1, "first"), (2, "second")] {
        let reservation = store
            .reserve(authority.clone(), None, || {
                (uuid::Uuid::new_v4().to_string(), sequence)
            })
            .unwrap();
        let record = store
            .admit_with_follow_up(&reservation, text, text, None, None, false)
            .unwrap();
        fixture.service.chat_send_ledger.admit_user(&record);
        records.push(record);
    }
    let (writes_tx, writes_rx) = mpsc::channel();
    let _boundary = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |record, text| {
            writes_tx
                .send((record.send_id.clone(), text.to_owned()))
                .unwrap();
            Ok(())
        }),
    );
    fixture.service.dispatch_ready_user_send(&records[1]);
    let first_write = writes_rx.recv_timeout(TEST_DEADLINE).unwrap();
    fixture.service.dispatch_ready_user_send(&records[0]);
    let second_write = writes_rx.recv_timeout(TEST_DEADLINE).unwrap();
    await_owner_idle(&fixture, &tab);
    assert_eq!(first_write, (records[0].send_id.clone(), "first".into()));
    assert_eq!(
        second_write,
        (records[1].send_id.clone(), "\n\nsecond".into())
    );
    assert!(matches!(
        writes_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
}

#[test]
fn ready_owner_advances_after_enqueue_while_prior_recovery_is_pending() {
    let (fixture, tab) = queue_fixture(true, None);
    mark_ready(&fixture, &tab);
    let (writes_tx, writes_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let first = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let _boundary = queued_boundary::QueuedSubmitBoundary::install(
        &tab,
        Arc::new(move |record, text, enqueued| {
            let record = record.unwrap();
            let wait = first.swap(false, Ordering::AcqRel);
            enqueued();
            writes_tx
                .send((record.send_id.clone(), text.to_owned()))
                .unwrap();
            if wait {
                release_rx.lock().recv_timeout(TEST_DEADLINE).unwrap();
            }
            Err(DeliveryError::SubmitUnconfirmed {
                tab: record.authority.tab_id.clone(),
                detail: "pending exact turn evidence".into(),
            })
        }),
    );
    let first = fixture
        .service
        .start_chat_send(&tab, "first".into(), None, None)
        .unwrap();
    assert_eq!(
        writes_rx.recv_timeout(TEST_DEADLINE).unwrap(),
        (first.send_id.clone(), "first".into())
    );
    let second = fixture
        .service
        .start_chat_send(&tab, "second".into(), None, None)
        .unwrap();
    let second_write = writes_rx.recv_timeout(TEST_DEADLINE);
    release_tx.send(()).unwrap();
    assert_eq!(
        second_write.unwrap(),
        (second.send_id.clone(), "\n\nsecond".into())
    );
    await_owner_idle(&fixture, &tab);
    assert!(matches!(
        writes_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
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
}

#[test]
fn ready_owner_preserves_intake_order_when_preparation_finishes_reversed() {
    let (fixture, tab) = queue_fixture(true, None);
    mark_ready(&fixture, &tab);
    let first = fixture.service.reserve_user_send(&tab, None).unwrap();
    let first_id = first.send_id.clone();
    let second = fixture.service.reserve_user_send(&tab, None).unwrap();
    let second_id = second.send_id.clone();
    let (writes_tx, writes_rx) = mpsc::channel();
    let _boundary = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |record, text| {
            writes_tx
                .send((record.send_id.clone(), text.to_owned()))
                .unwrap();
            Ok(())
        }),
    );
    fixture
        .service
        .prepare_and_admit_user_send(second, "second".into(), false)
        .unwrap();
    await_owner_idle(&fixture, &tab);
    assert!(matches!(
        writes_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    fixture
        .service
        .prepare_and_admit_user_send(first, "first".into(), false)
        .unwrap();
    await_owner_idle(&fixture, &tab);
    assert_eq!(
        writes_rx.recv_timeout(TEST_DEADLINE).unwrap(),
        (first_id, "first".into())
    );
    assert_eq!(
        writes_rx.recv_timeout(TEST_DEADLINE).unwrap(),
        (second_id, "\n\nsecond".into())
    );
    assert!(matches!(
        writes_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
}

#[test]
fn enqueued_payload_never_replays_after_a_late_zero_write_error() {
    let (fixture, tab) = queue_fixture(true, None);
    mark_ready(&fixture, &tab);
    let writes = Arc::new(AtomicUsize::new(0));
    let count = writes.clone();
    let _boundary = queued_boundary::QueuedSubmitBoundary::install(
        &tab,
        Arc::new(move |record, _, enqueued| {
            count.fetch_add(1, Ordering::AcqRel);
            enqueued();
            Err(DeliveryError::TabBusyRefused {
                tab: record.unwrap().authority.tab_id.clone(),
                detail: "late authority revocation".into(),
            })
        }),
    );
    let send = fixture
        .service
        .start_chat_send(&tab, "first".into(), None, None)
        .unwrap();
    await_owner_idle(&fixture, &tab);
    fixture.service.wake_parked_user_send(&tab);
    await_owner_idle(&fixture, &tab);
    assert_eq!(writes.load(Ordering::Acquire), 1);
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&send.send_id)
            .unwrap()
            .state,
        ChatSendState::Unknown
    );
    let reopened = UserSendStore::open(fixture.temp.path()).unwrap();
    assert!(reopened.records(&tab).unwrap().is_empty());
    assert!(reopened.original(&tab, &send.send_id).is_err());
}
