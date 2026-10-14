use super::*;

#[test]
fn held_persistence_keeps_mux_ledger_and_unrelated_admission_available_without_false_acceptance() {
    let (fixture, tab) = queue_fixture(true, None);
    let (held_tx, held_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Arc::new(Mutex::new(release_rx));
    fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .set_persistence_boundary(Some(Arc::new(move |stage| {
            if stage == "index" {
                held_tx.send(()).unwrap();
                release_rx.lock().recv_timeout(TEST_DEADLINE).unwrap();
            }
            Ok(())
        })));
    let svc = fixture.service.clone();
    let reserved = svc
        .reserve_user_send(&tab, Some("client-a".into()))
        .unwrap();
    let send_id = reserved.send_id.clone();
    let (accepted_tx, accepted_rx) = mpsc::channel();
    let admission = std::thread::spawn(move || {
        let result = svc.prepare_and_admit_user_send(reserved, FIRST_PAYLOAD.into(), false);
        accepted_tx.send(result.is_ok()).unwrap();
    });
    held_rx.recv_timeout(TEST_DEADLINE).unwrap();
    assert!(matches!(
        accepted_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    let (probe_tx, probe_rx) = mpsc::channel();
    let svc = fixture.service.clone();
    let probe_tab = tab.clone();
    let probe_id = send_id.clone();
    let probe = std::thread::spawn(move || {
        let mux_available = svc.mux.try_write().is_some();
        let unpublished = svc.chat_send_ledger.status(&probe_id).is_none();
        let reserved = svc
            .reserve_user_send(&probe_tab, Some("client-b".into()))
            .unwrap();
        probe_tx
            .send((mux_available, unpublished, reserved.ledger_sequence))
            .unwrap();
    });
    let observed = probe_rx.recv_timeout(TEST_DEADLINE);
    release_tx.send(()).unwrap();
    let (mux_available, unpublished, sequence) = observed.unwrap();
    assert!(mux_available);
    assert!(unpublished);
    assert!(sequence > 0);
    probe.join().unwrap();
    assert!(accepted_rx.recv_timeout(TEST_DEADLINE).unwrap());
    admission.join().unwrap();
    fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .set_persistence_boundary(None);
    assert_eq!(
        fixture
            .service
            .pending_delivery_payloads
            .user_sends
            .original(&tab, &send_id)
            .unwrap(),
        FIRST_PAYLOAD
    );
}

#[test]
fn close_during_durable_claim_revokes_before_any_writer_handoff_and_preserves_both() {
    let (fixture, tab) = queue_fixture(true, None);
    let first = start_current_chat_send(&fixture, &tab, FIRST_PAYLOAD, "client-a");
    let second = start_current_chat_send(&fixture, &tab, SECOND_PAYLOAD, "client-b");
    let writes = Arc::new(AtomicUsize::new(0));
    let count = writes.clone();
    let _boundary = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |_, _| {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
    );
    let (held_tx, held_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let once = Arc::new(std::sync::atomic::AtomicBool::new(false));
    fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .set_persistence_boundary(Some(Arc::new(move |stage| {
            if stage == "index" && !once.swap(true, Ordering::SeqCst) {
                held_tx.send(()).unwrap();
                release_rx.lock().recv_timeout(TEST_DEADLINE).unwrap();
            }
            Ok(())
        })));
    mark_ready(&fixture, &tab);
    fixture.service.wake_parked_user_send(&tab);
    held_rx.recv_timeout(TEST_DEADLINE).unwrap();
    fixture
        .service
        .prepare_tab_close(&tab, crate::multiplexer::types::TabCloseReason::User)
        .unwrap();
    assert!(fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .tab(&tab)
        .revoked
        .load(Ordering::Acquire));
    let (listener, settled) = settled_listener(&fixture, &tab, &first.send_id);
    release_tx.send(()).unwrap();
    assert_eq!(
        settled.recv_timeout(TEST_DEADLINE).unwrap()["send_id"],
        first.send_id
    );
    fixture._app.unlisten(listener);
    fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .set_persistence_boundary(None);
    fixture.service.wake_parked_user_send(&tab);
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    assert_reopened_originals(&fixture, &[first, second]);
}

#[test]
fn production_preparation_archives_exact_owned_body_and_preserves_it_after_pad_removal() {
    let (fixture, tab) = queue_fixture(true, Some(8));
    let ticket = fixture
        .service
        .reserve_user_send(&tab, Some("opaque-client".into()))
        .unwrap();
    let result = fixture
        .service
        .prepare_and_admit_user_send(ticket, FIRST_PAYLOAD.into(), false)
        .unwrap();
    let body_path = std::path::PathBuf::from(result.pad_file.unwrap());
    let expected = fixture
        .service
        .ensure_agent_pad(&tab)
        .unwrap()
        .path()
        .to_path_buf();
    assert!(!body_path.starts_with(&expected));
    assert_eq!(std::fs::read(&body_path).unwrap(), FIRST_PAYLOAD.as_bytes());
    fixture.service.revoke_agent_pad(&tab).unwrap();
    let reopened = UserSendStore::open(fixture.temp.path()).unwrap();
    let record = reopened.records(&tab).unwrap().remove(0);
    assert_eq!(
        reopened.original(&tab, &result.status.send_id).unwrap(),
        FIRST_PAYLOAD
    );
    assert_eq!(
        std::fs::read(reopened.body_path(&record).unwrap().unwrap()).unwrap(),
        FIRST_PAYLOAD.as_bytes()
    );
    assert!(reopened
        .prepared(&record)
        .unwrap()
        .ends_with(&codeterm_paths::strip_verbatim_prefix(
            body_path.to_str().unwrap()
        )));
}

#[test]
fn reversed_service_preparation_keeps_intake_order_before_first_handoff() {
    let (fixture, tab) = queue_fixture(false, None);
    let first = fixture
        .service
        .reserve_user_send(&tab, Some("client-a".into()))
        .unwrap();
    let second = fixture
        .service
        .reserve_user_send(&tab, Some("client-b".into()))
        .unwrap();
    let (writes_tx, writes_rx) = mpsc::channel();
    let _boundary = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |record, text| {
            writes_tx
                .send((record.send_id.clone(), text.to_string()))
                .unwrap();
            Err(DeliveryError::SubmitUnconfirmed {
                tab: record.authority.tab_id.clone(),
                detail: "opaque-uncertainty".into(),
            })
        }),
    );
    mark_ready(&fixture, &tab);
    let b = fixture
        .service
        .prepare_and_admit_user_send(second, SECOND_PAYLOAD.into(), false)
        .unwrap();
    await_owner_idle(&fixture, &tab);
    assert!(matches!(
        writes_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    let a = fixture
        .service
        .prepare_and_admit_user_send(first, FIRST_PAYLOAD.into(), false)
        .unwrap();
    let (first_write, first_text) = writes_rx.recv_timeout(TEST_DEADLINE).unwrap();
    assert_eq!(first_write, a.status.send_id);
    assert!(matches!(
        writes_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
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
    assert_eq!(first_confirmation[0].send_id, a.status.send_id);
    let (second_write, second_text) = writes_rx.recv_timeout(TEST_DEADLINE).unwrap();
    assert_eq!(second_write, b.status.send_id);
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
    assert_eq!(second_confirmation[0].send_id, b.status.send_id);
    await_owner_idle(&fixture, &tab);
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
            .status(&a.status.send_id)
            .unwrap()
            .state,
        ChatSendState::Sent
    );
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&b.status.send_id)
            .unwrap()
            .state,
        ChatSendState::Sent
    );
}

#[test]
fn refused_durable_service_admission_never_publishes_acceptance_or_evicts_first() {
    let (fixture, tab) = queue_fixture(true, None);
    let accepted = start_current_chat_send(&fixture, &tab, FIRST_PAYLOAD, "client-a");
    fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .set_persistence_boundary(Some(Arc::new(|stage| {
            if stage == "index" {
                Err("opaque-index-error".into())
            } else {
                Ok(())
            }
        })));
    let reserved = fixture
        .service
        .reserve_user_send(&tab, Some("client-b".into()))
        .unwrap();
    let refused_id = reserved.send_id.clone();
    let error = fixture
        .service
        .prepare_and_admit_user_send(reserved, SECOND_PAYLOAD.into(), false)
        .err()
        .unwrap();
    assert!(error.starts_with("admission_unknown:"));
    assert!(fixture
        .service
        .chat_send_ledger
        .status(&refused_id)
        .is_none());
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&accepted.send_id)
            .unwrap()
            .state,
        ChatSendState::Pending
    );
    let reopened = UserSendStore::open(fixture.temp.path()).unwrap();
    assert_eq!(reopened.records(&tab).unwrap().len(), 1);
    assert_eq!(
        reopened.original(&tab, &accepted.send_id).unwrap(),
        FIRST_PAYLOAD
    );
}
