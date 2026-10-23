use super::*;
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn blueprint_v2_call_durable_emit_loser_parks_without_receipt_io_and_winner_wakes_next_result() {
    let (fixture, tab, id, _) = held();
    let writes = fixture.record_agent_deliveries(&tab);
    let owner = fixture.service.pending_delivery_payloads.agent_deliveries.clone();
    owner.mark_terminal_failure(&tab, &id, "recipient_gone", None).unwrap();
    let mut envelope = MessageEnvelope::codeterm_reminder("blueprint:later-result");
    envelope.durable_delivery = Some(fixture.service.durable_delivery("later-run", &tab));
    let Delivery::Pending { .. } = Messenger::deliver(
        &fixture.service, &tab, envelope, "later body",
    ).unwrap() else { panic!("later result is durably admitted"); };
    owner.state_changed(&tab);
    assert!(owner.start_worker(&tab));
    fixture.resume_recorded_agent_recipient(&tab, "saved-session", "first-launch");
    let record = owner.pending_head(&tab).unwrap().unwrap();
    let payload: AgentDeliveryPayload = serde_json::from_str(
        &owner.payload(&tab, &record).unwrap(),
    ).unwrap();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = parking_lot::Mutex::new(release_rx);
    let expected_id = id.clone();
    fixture.ui.listen("agent-delivery-status", move |event| {
        if event["delivery_id"] == expected_id && event["status"] == "failed" {
            entered_tx.send(()).unwrap();
            release_rx.lock().recv_timeout(Duration::from_secs(5)).unwrap();
        }
    });
    let service = fixture.service.clone();
    let winner_owner = owner.clone();
    let winner_record = record.clone();
    let winner_payload = payload.clone();
    let target = tab.clone();
    let winner = std::thread::spawn(move || {
        let _claim = service.pty_ownership.acquire(&target);
        service.fail_durable_delivery(
            &target, &winner_owner, &winner_record, &winner_payload, "recipient_gone",
        )
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let pad = fixture.service.ensure_agent_pad(&tab).unwrap();
    let inbox = pad.path().join("inbox");
    let saved_inbox = pad.path().join("paused-receipt-inbox");
    let receipt = inbox.join(format!("delivery-{id}.json"));
    let bytes = std::fs::read(&receipt).unwrap();
    std::fs::rename(&inbox, &saved_inbox).unwrap();
    std::fs::write(&inbox, "receipt writes must not be attempted by a loser").unwrap();
    assert_eq!(fixture.service.fail_durable_delivery(
        &tab, &owner, &record, &payload, "delivery_ttl_expired",
    ), Ok(SettlementOutcome::Held));
    std::fs::remove_file(&inbox).unwrap();
    std::fs::rename(saved_inbox, &inbox).unwrap();
    owner.finish_worker(&tab);
    fixture.service.wake_agent_deliveries_on_state_change(&tab);
    writes.wait_for_flush();
    assert_eq!(owner.pending_head(&tab).unwrap().unwrap().id, id);
    assert!(!owner.start_worker(&tab), "losing worker is parked, not spinning");
    assert!(writes.texts.lock().is_empty());
    assert_eq!(std::fs::read(&receipt).unwrap(), bytes);
    release_tx.send(()).unwrap();
    assert_eq!(winner.join().unwrap().unwrap(), SettlementOutcome::Settled);
    writes.wait_for_flush();
    assert_eq!(writes.texts.lock().as_slice(),
        ["Original run: later-run; timestamp: 1000\nlater body"]);
    assert!(!fixture.has_pending_agent_delivery(&tab));
    assert_eq!(fixture.ui.0.lock().iter().filter(|(kind, event)|
        kind == "agent-delivery-status" && event["delivery_id"] == id
            && event["status"] == "failed").count(), 1);
}

#[test]
fn blueprint_v2_call_durable_notifier_panic_releases_emit_claim_and_recovers_without_restart() {
    let (fixture, tab, id, _) = held();
    let writes = fixture.record_agent_deliveries(&tab);
    let owner = &fixture.service.pending_delivery_payloads.agent_deliveries;
    let record = owner.pending_head(&tab).unwrap().unwrap();
    let payload: AgentDeliveryPayload =
        serde_json::from_str(&owner.payload(&tab, &record).unwrap()).unwrap();
    let listener = fixture.ui.listen("notification-created", |_| {
        panic!("injected notifier failure after acquiring the emit claim");
    });
    let failed = fixture.service.fail_durable_delivery(
        &tab, owner, &record, &payload, "recipient_gone",
    );
    assert!(failed.unwrap_err().contains("notifier panicked"));
    fixture.ui.unlisten(listener);
    assert!(fixture.has_pending_agent_delivery(&tab));
    assert!(!fixture.ui.0.lock().iter().any(|(kind, event)|
        kind == "agent-delivery-status" && event["delivery_id"] == id
            && event["status"] == "failed"));
    fixture.service.wake_agent_deliveries_on_state_change(&tab);
    writes.wait_for_flush();
    assert!(!fixture.has_pending_agent_delivery(&tab));
    assert!(writes.texts.lock().is_empty());
    assert_eq!(fixture.ui.0.lock().iter().filter(|(kind, event)|
        kind == "agent-delivery-status" && event["delivery_id"] == id
            && event["status"] == "failed").count(), 1);
    let inbox = fixture.service.ensure_agent_pad(&tab).unwrap().path().join("inbox");
    let files = std::fs::read_dir(inbox).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(files.len(), 1);
    let receipt: serde_json::Value = serde_json::from_slice(&std::fs::read(files[0].path()).unwrap()).unwrap();
    assert_eq!(receipt["code"], "recipient_gone");
}

#[test]
fn blueprint_v2_call_durable_receipt_error_rolls_back_emit_claim_before_retry() {
    let (fixture, tab, id, _) = held();
    let owner = &fixture.service.pending_delivery_payloads.agent_deliveries;
    let record = owner.pending_head(&tab).unwrap().unwrap();
    let payload: AgentDeliveryPayload =
        serde_json::from_str(&owner.payload(&tab, &record).unwrap()).unwrap();
    let inbox = fixture.service.ensure_agent_pad(&tab).unwrap().path().join("inbox");
    std::fs::write(&inbox, "injected receipt directory failure").unwrap();
    assert!(fixture.service.fail_durable_delivery(
        &tab, owner, &record, &payload, "recipient_gone",
    ).is_err());
    assert!(fixture.has_pending_agent_delivery(&tab));
    std::fs::remove_file(&inbox).unwrap();
    assert_eq!(fixture.service.fail_durable_delivery(
        &tab, owner, &record, &payload, "recipient_gone",
    ), Ok(SettlementOutcome::Settled));
    assert!(!fixture.has_pending_agent_delivery(&tab));
}
