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
