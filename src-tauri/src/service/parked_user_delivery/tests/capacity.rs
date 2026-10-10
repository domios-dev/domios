use super::*;

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires the shared interactive desktop ServiceContext fixture"
)]
fn full_user_owner_refuses_new_admission_without_consuming_backend_ledger_capacity() {
    let (fixture, tab) = queue_fixture(true, None);
    let accepted = (0..crate::service::pending_delivery_payloads::user_sends::MAX_UNSETTLED)
        .map(|index| {
            start_current_chat_send(
                &fixture,
                &tab,
                FIRST_PAYLOAD,
                &format!("opaque-client-{index}"),
            )
        })
        .collect::<Vec<_>>();
    let refusal = fixture
        .service
        .start_chat_send(
            &tab,
            SECOND_PAYLOAD.into(),
            None,
            Some("opaque-refused-client".into()),
        )
        .unwrap_err();
    assert!(refusal.contains("capacity"));
    assert!(fixture
        .service
        .chat_send_ledger
        .try_start_scoped_with_normalization(
            "opaque-backend-ledger",
            "opaque-backend",
            None,
            None,
            Default::default(),
        )
        .is_ok());
    let reopened = UserSendStore::open(fixture.temp.path()).unwrap();
    assert_eq!(reopened.records(&tab).unwrap().len(), accepted.len());
    for status in accepted {
        assert_eq!(
            reopened.original(&tab, &status.send_id).unwrap(),
            FIRST_PAYLOAD
        );
    }
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires the shared interactive desktop ServiceContext fixture"
)]
fn zero_write_refusal_retries_head_then_writes_following_sends_after_uncertainty() {
    for reason in [
        UserWake::Housekeeping,
        UserWake::Readiness,
        UserWake::Admission,
    ] {
        let (fixture, tab) = queue_fixture(false, None);
        let first = start_current_chat_send(&fixture, &tab, FIRST_PAYLOAD, "client-a");
        let second = start_current_chat_send(&fixture, &tab, SECOND_PAYLOAD, "client-b");
        let attempts = Arc::new(AtomicUsize::new(0));
        let count = attempts.clone();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Arc::new(Mutex::new(release_rx));
        let _submit = UserSubmitBoundary::install(
            &tab,
            Arc::new(move |record, _| {
                let attempt = count.fetch_add(1, Ordering::AcqRel);
                entered_tx.send((attempt, record.send_id.clone())).unwrap();
                if attempt == 0 {
                    release_rx.lock().recv_timeout(TEST_DEADLINE).unwrap();
                    Err(DeliveryError::TabBusyRefused {
                        tab: record.authority.tab_id.clone(),
                        detail: "opaque-zero-write".into(),
                    })
                } else {
                    Err(DeliveryError::SubmitUnconfirmed {
                        tab: record.authority.tab_id.clone(),
                        detail: "opaque-postwrite".into(),
                    })
                }
            }),
        );
        let (idle_tx, idle_rx) = mpsc::sync_channel(1);
        let count = attempts.clone();
        let _idle = OwnerIdleBoundary::install(
            &tab,
            Arc::new(move || {
                if count.load(Ordering::Acquire) > 0 {
                    let _ = idle_tx.try_send(());
                }
            }),
        );
        mark_ready(&fixture, &tab);
        fixture.service.wake_parked_user_send(&tab);
        assert_eq!(
            entered_rx.recv_timeout(TEST_DEADLINE).unwrap(),
            (0, first.send_id.clone())
        );
        let additional = if reason == UserWake::Admission {
            let ticket = fixture
                .service
                .reserve_user_send(&tab, Some("client-c".into()))
                .unwrap();
            Some(
                fixture
                    .service
                    .prepare_and_admit_user_send(ticket, SECOND_PAYLOAD.into(), false)
                    .unwrap()
                    .status,
            )
        } else {
            fixture.service.wake_user_owner(&tab, reason);
            None
        };
        release_tx.send(()).unwrap();
        idle_rx.recv_timeout(TEST_DEADLINE).unwrap();
        let mut expected_follow_ups = match reason {
            UserWake::Housekeeping => Vec::new(),
            UserWake::Readiness => vec![first.send_id.clone(), second.send_id.clone()],
            UserWake::Admission => vec![
                first.send_id.clone(),
                second.send_id.clone(),
                additional.as_ref().unwrap().send_id.clone(),
            ],
        };
        if reason == UserWake::Housekeeping {
            assert_eq!(attempts.load(Ordering::Acquire), 1);
            let records = fixture
                .service
                .pending_delivery_payloads
                .user_sends
                .records(&tab)
                .unwrap();
            assert_eq!(records[0].state, UserSendState::Queued);
            assert_eq!(records[1].state, UserSendState::Queued);
            fixture.service.wake_parked_user_send(&tab);
            idle_rx.recv_timeout(TEST_DEADLINE).unwrap();
            expected_follow_ups = vec![first.send_id.clone(), second.send_id.clone()];
        }
        assert_eq!(
            attempts.load(Ordering::Acquire),
            expected_follow_ups.len() + 1
        );
        for (attempt, send_id) in expected_follow_ups.iter().enumerate() {
            assert_eq!(
                entered_rx.recv_timeout(TEST_DEADLINE).unwrap(),
                (attempt + 1, send_id.clone())
            );
        }
        let records = fixture
            .service
            .pending_delivery_payloads
            .user_sends
            .records(&tab)
            .unwrap();
        assert!(records.is_empty());
        for send_id in std::iter::once(&first.send_id)
            .chain(std::iter::once(&second.send_id))
            .chain(additional.iter().map(|status| &status.send_id))
        {
            assert_eq!(
                fixture
                    .service
                    .chat_send_ledger
                    .status(send_id)
                    .unwrap()
                    .state,
                ChatSendState::Unknown
            );
        }
        let reopened = UserSendStore::open(fixture.temp.path()).unwrap();
        assert!(reopened.records(&tab).unwrap().is_empty());
        for send_id in std::iter::once(&first.send_id)
            .chain(std::iter::once(&second.send_id))
            .chain(additional.iter().map(|status| &status.send_id))
        {
            assert!(reopened.original(&tab, send_id).is_err());
        }
    }
}
