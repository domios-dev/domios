use super::*;

#[test]
fn lazy_session_provider_writes_every_send_after_agent_readiness() {
    let (fixture, tab) = queue_fixture_with_session_mode(false, None, true);
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab).unwrap();
        tab.agent_session_id = None;
        tab.reported_session_binding_nonce = None;
    }
    mark_ready(&fixture, &tab);
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let attempts = Arc::new(AtomicUsize::new(0));
    let count = attempts.clone();
    let wait_for_release = release_rx.clone();
    let writes = Arc::new(Mutex::new(Vec::new()));
    let observed = writes.clone();
    let _submit = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |record, text| {
            observed
                .lock()
                .push((record.send_id.clone(), text.to_string()));
            if count.fetch_add(1, Ordering::AcqRel) == 0 {
                entered_tx.send(record.send_id.clone()).unwrap();
                wait_for_release.lock().recv_timeout(TEST_DEADLINE).unwrap();
            }
            Err(DeliveryError::SubmitUnconfirmed {
                tab: record.authority.tab_id.clone(),
                detail: "opaque-postwrite".into(),
            })
        }),
    );
    let first_reservation = fixture
        .service
        .reserve_user_send(&tab, Some("lazy-client-0".into()))
        .unwrap();
    let first = fixture
        .service
        .prepare_and_admit_user_send(first_reservation, "first lazy send".into(), false)
        .unwrap()
        .status;
    assert_eq!(
        entered_rx.recv_timeout(TEST_DEADLINE).unwrap(),
        first.send_id
    );
    let later = ["second lazy send", "third lazy send"]
        .iter()
        .enumerate()
        .map(|(index, payload)| {
            let reservation = fixture
                .service
                .reserve_user_send(&tab, Some(format!("lazy-client-{}", index + 1)))
                .unwrap();
            fixture
                .service
                .prepare_and_admit_user_send(reservation, (*payload).into(), false)
                .unwrap()
                .status
        })
        .collect::<Vec<_>>();
    assert!(fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .records(&tab)
        .unwrap()
        .iter()
        .all(|record| !record.held_before_readiness));
    release_tx.send(()).unwrap();
    await_owner_idle(&fixture, &tab);

    let sends = std::iter::once(first).chain(later).collect::<Vec<_>>();
    let writes = writes.lock().clone();
    assert_eq!(
        writes
            .iter()
            .map(|(send_id, _)| send_id.as_str())
            .collect::<Vec<_>>(),
        sends
            .iter()
            .map(|status| status.send_id.as_str())
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        writes
            .iter()
            .map(|(_, text)| text.as_str())
            .collect::<Vec<_>>(),
        [
            "first lazy send",
            "\n\nsecond lazy send",
            "\n\nthird lazy send"
        ],
    );
    assert!(fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .records(&tab)
        .unwrap()
        .is_empty());
    assert!(sends.iter().all(|status| {
        fixture
            .service
            .chat_send_ledger
            .status(&status.send_id)
            .unwrap()
            .state
            == ChatSendState::Unknown
    }));
    assert_eq!(attempts.load(Ordering::Acquire), sends.len());
}
