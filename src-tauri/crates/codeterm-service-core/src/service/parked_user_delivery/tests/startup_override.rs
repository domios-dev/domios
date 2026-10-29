use super::*;

#[test]
fn explicit_override_flushes_held_sends_once_in_order_and_binding_retires_startup() {
    let (fixture, tab) = queue_fixture(true, None);
    let first = admit_current_ledger(&fixture.service, &tab, FIRST_PAYLOAD, "first-held");
    refuse_and_park_current_send(&fixture.service, first.clone(), FIRST_PAYLOAD);
    let second = admit_current_ledger(&fixture.service, &tab, SECOND_PAYLOAD, "second-held");
    refuse_and_park_current_send(&fixture.service, second.clone(), SECOND_PAYLOAD);
    attach_fixture_pty(&fixture, &tab);
    let (sent, received) = mpsc::channel();
    let _boundary = UserSubmitBoundary::install(&tab, Arc::new(move |record, text| {
        sent.send((record.send_id.clone(), text.to_string())).unwrap();
        Err(DeliveryError::SubmitUnconfirmed {
            tab: record.authority.tab_id.clone(), detail: "own-session evidence pending".into(),
        })
    }));
    fixture.service.drain_user_owner(&tab).unwrap();
    assert!(received.try_recv().is_err());
    let authority = fixture.service.user_send_authority(&tab).unwrap();
    assert!(fixture.service.override_user_startup(&tab, "retired-launch").is_err());
    assert!(!fixture.service.user_authority_is_ready(&authority));
    fixture.service.override_user_startup(&tab, &authority.binding_nonce).unwrap();
    await_owner_idle(&fixture, &tab);
    assert_eq!(received.recv_timeout(TEST_DEADLINE).unwrap(), (first.send_id, FIRST_PAYLOAD.to_owned()));
    assert_eq!(received.recv_timeout(TEST_DEADLINE).unwrap(), (second.send_id, format!("\n\n{SECOND_PAYLOAD}")));
    fixture.service.override_user_startup(&tab, &authority.binding_nonce).unwrap();
    fixture.service.drain_user_owner(&tab).unwrap();
    assert!(received.try_recv().is_err());
    assert_eq!(fixture.service.mux.read().get_tab(&tab).unwrap().boot_stage,
        Some(codeterm_types::pty::BootStage::LaunchBindingUnconfirmed));
    fixture.service.mux.write().get_tab_mut(&tab).unwrap().reported_session_binding_nonce = Some(authority.binding_nonce);
    fixture.service.reconcile_boot_stage_with_session_binding(&tab);
    assert_eq!(fixture.service.mux.read().get_tab(&tab).unwrap().boot_stage,
        Some(codeterm_types::pty::BootStage::AgentReady));
    fixture.service.drain_user_owner(&tab).unwrap();
    assert!(received.try_recv().is_err());
    assert!(fixture.service.pending_delivery_payloads.user_sends.records(&tab).unwrap().is_empty());
}
