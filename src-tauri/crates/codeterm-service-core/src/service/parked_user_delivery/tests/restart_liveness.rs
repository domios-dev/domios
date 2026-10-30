use super::*;

fn admit_without_dispatch(fixture: &ServiceFixture, tab: &str, payload: &str) -> UserSendRecord {
    let store = &fixture.service.pending_delivery_payloads.user_sends;
    let reservation = store
        .reserve(
            fixture.service.user_send_authority(tab).unwrap(),
            Some(uuid::Uuid::new_v4().to_string()),
            || fixture.service.chat_send_ledger.reserve_identity(),
        )
        .unwrap();
    let record = store.admit(&reservation, payload, payload, None).unwrap();
    fixture.service.chat_send_ledger.admit_user(&record);
    record
}

#[test]
fn ease_retained_status_reads_wake_only_held_input_without_readmitting_or_replaying() {
    use std::collections::BTreeSet;

    let (fixture, tab) = queue_fixture(true, None);
    attach_fixture_pty(&fixture, &tab);
    let store = &fixture.service.pending_delivery_payloads.user_sends;
    let written = admit_without_dispatch(&fixture, &tab, "opaque-written");
    let uncertain = admit_without_dispatch(&fixture, &tab, "opaque-uncertain");
    let held = admit_without_dispatch(&fixture, &tab, "opaque-held");
    for record in [&written, &uncertain] {
        assert!(store.claim(record, "opaque-session").unwrap());
        fixture
            .service
            .chat_send_ledger
            .mark_user_written(&record.send_id)
            .unwrap();
        store.finish_handoff(record, false).unwrap();
    }
    fixture
        .service
        .chat_send_ledger
        .finish(
            &uncertain.send_id,
            Err("submit_unconfirmed: opaque evidence pending".into()),
        )
        .unwrap();
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&written.send_id)
            .unwrap()
            .state,
        ChatSendState::Written
    );
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&uncertain.send_id)
            .unwrap()
            .state,
        ChatSendState::Unknown
    );
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&held.send_id)
            .unwrap()
            .state,
        ChatSendState::Pending
    );
    let identities = [
        written.send_id.clone(),
        uncertain.send_id.clone(),
        held.send_id.clone(),
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    let sequences = fixture
        .service
        .chat_send_ledger
        .reconcile(&tab)
        .into_iter()
        .map(|status| (status.send_id, status.sequence))
        .collect::<std::collections::BTreeMap<_, _>>();
    let writes = Arc::new(Mutex::new(Vec::new()));
    let observed = writes.clone();
    let _boundary = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |record, _| {
            observed.lock().push(record.send_id.clone());
            Err(DeliveryError::SubmitUnconfirmed {
                tab: record.authority.tab_id.clone(),
                detail: "opaque evidence pending".into(),
            })
        }),
    );
    for id in &identities {
        assert_eq!(
            fixture
                .service
                .inspect_retained_send(id)
                .unwrap()
                .unwrap()
                .send_id,
            *id
        );
    }
    await_owner_idle(&fixture, &tab);
    assert!(writes.lock().is_empty());
    mark_ready(&fixture, &tab);
    fixture
        .service
        .inspect_retained_send(&held.send_id)
        .unwrap();
    await_owner_idle(&fixture, &tab);
    for _ in 0..3 {
        for id in &identities {
            fixture.service.inspect_retained_send(id).unwrap();
        }
        assert!(fixture
            .service
            .inspect_retained_send("not-admitted")
            .unwrap()
            .is_none());
        await_owner_idle(&fixture, &tab);
    }
    assert_eq!(*writes.lock(), vec![held.send_id.clone()]);
    let retained_ids = store
        .records(&tab)
        .unwrap()
        .into_iter()
        .map(|record| record.send_id)
        .chain(
            store
                .receipts(&tab)
                .unwrap()
                .into_iter()
                .map(|receipt| receipt.send_id),
        )
        .collect::<BTreeSet<_>>();
    assert_eq!(retained_ids, identities);
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .reconcile(&tab)
            .into_iter()
            .map(|status| (status.send_id, status.sequence))
            .collect::<std::collections::BTreeMap<_, _>>(),
        sequences
    );
}

#[test]
fn restored_claim_settles_as_uncertainty_without_blocking_later_sends() {
    let (fixture, tab) = queue_fixture(true, None);
    mark_ready(&fixture, &tab);
    let previous = UserSendStore::open(fixture.temp.path()).unwrap();
    let reservation = previous
        .reserve(
            fixture.service.user_send_authority(&tab).unwrap(),
            None,
            || fixture.service.chat_send_ledger.reserve_identity(),
        )
        .unwrap();
    let claimed = previous
        .admit(&reservation, "orphaned-claim", "orphaned-claim", None)
        .unwrap();
    drop(reservation);
    assert!(previous.claim(&claimed, "opaque-session").unwrap());
    let later = admit_without_dispatch(&fixture, &tab, "later-send");
    let mut written = Vec::new();

    fixture
        .service
        .drain_user_owner_with(&tab, |record, _, _, _| {
            fixture
                .service
                .chat_send_ledger
                .mark_user_written(&record.send_id)
                .unwrap();
            written.push(record.send_id.clone());
            Ok(UserWrite::Finished)
        })
        .unwrap();

    assert_eq!(written, [later.send_id]);
    let status = fixture
        .service
        .chat_send_ledger
        .status(&claimed.send_id)
        .unwrap();
    assert_eq!(status.state, ChatSendState::Unknown);
    assert_eq!(status.error, None);
    let reopened = UserSendStore::open(fixture.temp.path()).unwrap();
    assert!(reopened.head(&tab).unwrap().is_none());
    assert_eq!(
        reopened
            .receipts(&tab)
            .unwrap()
            .iter()
            .find(|receipt| receipt.send_id == claimed.send_id)
            .unwrap()
            .state,
        crate::service::pending_delivery_payloads::user_sends::continuity::ReceiptState::Uncertain
    );
    fixture
        .service
        .drain_user_owner_with(&tab, |_, _, _, _| panic!("claimed input must not replay"))
        .unwrap();
}

#[test]
fn restart_ready_launch_terminalizes_stale_head_and_delivers_later_sends_in_one_pass() {
    let (fixture, tab) = queue_fixture(true, None);
    {
        let mut mux = fixture.service.mux.write();
        let launch = mux.get_tab_mut(&tab).unwrap();
        launch.agent_session_id = Some("old-session".into());
        launch.reported_session_binding_nonce = launch.session_binding_nonce.clone();
        launch.boot_stage = Some(codeterm_types::pty::BootStage::LaunchWritten);
    }
    attach_fixture_pty(&fixture, &tab);
    let stuck = admit_without_dispatch(&fixture, &tab, "stuck-never-written");
    fixture
        .service
        .drain_user_owner_with(&tab, |_, _, _, _| panic!("launch A is not ready"))
        .unwrap();
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&stuck.send_id)
            .unwrap()
            .pending_reason
            .as_deref(),
        Some("exact_binding_present_stage_pending")
    );
    {
        let mut mux = fixture.service.mux.write();
        let launch = mux.get_tab_mut(&tab).unwrap();
        launch.pty_id = Some("replacement-pty".into());
        launch.session_binding_nonce = Some("replacement-nonce".into());
        launch.agent_session_id = Some("opaque-session".into());
    }
    mark_ready(&fixture, &tab);
    let mut changed_authority = stuck.authority.clone();
    changed_authority.pty_session_generation = Some(0);
    let store = &fixture.service.pending_delivery_payloads.user_sends;
    store
        .rebind_queued_authority(&stuck, changed_authority)
        .unwrap()
        .unwrap();
    assert!(!store.discard_queued_from_ended_launch(&stuck).unwrap());
    fixture.service.settle_ended_user_head(&stuck).unwrap();
    let second = admit_without_dispatch(&fixture, &tab, "later-two");
    let third = admit_without_dispatch(&fixture, &tab, "later-three");
    let mut written = Vec::new();
    fixture
        .service
        .drain_user_owner_with(&tab, |record, text, _, _| {
            fixture
                .service
                .chat_send_ledger
                .mark_user_written(&record.send_id)
                .unwrap();
            written.push((record.send_id.clone(), text.to_owned()));
            Ok(UserWrite::Finished)
        })
        .unwrap();
    assert_eq!(
        written.iter().map(|(id, _)| id).collect::<Vec<_>>(),
        [&second.send_id, &third.send_id]
    );
    let failed = fixture
        .service
        .chat_send_ledger
        .status(&stuck.send_id)
        .unwrap();
    assert_eq!(failed.state, ChatSendState::Failed);
    assert_eq!(failed.error.as_deref(), Some("launch_ended_before_write"));
    assert_eq!(failed.pending_reason, None);
    for (index, (send_id, text)) in written.iter().enumerate() {
        append_turn(&fixture, text);
        let turn = observed_turn(&fixture, &tab, &format!("turn-{index}"), text);
        let confirmed = deliver_turn(&fixture, &tab, "opaque-session", "replacement-nonce", &turn);
        assert_eq!(confirmed.len(), 1);
        assert_eq!(&confirmed[0].send_id, send_id);
        assert_eq!(confirmed[0].state, ChatSendState::Sent);
    }
    let reopened = UserSendStore::open(fixture.temp.path()).unwrap();
    assert!(reopened.head(&tab).unwrap().is_none());
    assert_eq!(
        reopened
            .receipts(&tab)
            .unwrap()
            .iter()
            .find(|receipt| receipt.send_id == stuck.send_id)
            .unwrap()
            .state,
        crate::service::pending_delivery_payloads::user_sends::continuity::ReceiptState::Failed
    );
    fixture
        .service
        .drain_user_owner_with(&tab, |_, _, _, _| panic!("settled input must not replay"))
        .unwrap();
}
