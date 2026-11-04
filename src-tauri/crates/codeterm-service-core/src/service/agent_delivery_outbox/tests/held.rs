use super::*;
use crate::service::chat_send_transaction::ChatSendState;
use crate::service::pending_delivery_payloads::agent_deliveries::agent_delivery_send_id;

fn admit(fixture: &ServiceFixture, tab: &str, body: &str) -> String {
    let mut message = payload();
    message.transport = message.transport.with_body(body.into());
    let Delivery::Pending { delivery_id } = fixture
        .service
        .deliver_agent_payload(tab, message, body)
        .unwrap()
    else {
        panic!("unbound recipient must hold input");
    };
    agent_delivery_send_id(tab, &delivery_id)
}

#[test]
fn cancelled_held_send_never_writes_and_later_sends_flush_in_order_once() {
    let (fixture, tab) = fixture(Some("saved-session"), false);
    let writes = fixture.record_agent_deliveries(&tab);
    let cancelled = admit(&fixture, &tab, "cancelled fragment");
    let first = admit(&fixture, &tab, "first fragment");
    let second = admit(&fixture, &tab, "second fragment");
    fixture.service.wake_agent_deliveries_on_state_change(&tab);
    writes.wait_for_flush();
    assert!(writes.texts.lock().is_empty());
    let held = fixture
        .service
        .inspect_retained_send(&first)
        .unwrap()
        .unwrap();
    assert_eq!(held.state, ChatSendState::Pending);
    assert!(held
        .pending_reason
        .as_deref()
        .unwrap()
        .contains("session binding"));
    assert!(held
        .pending_reason
        .as_deref()
        .unwrap()
        .contains("send-cancel"));
    let receipt = fixture
        .service
        .cancel_retained_agent_send(&cancelled)
        .unwrap();
    assert_eq!(receipt.state, ChatSendState::Failed);
    assert_eq!(receipt.error.as_deref(), Some("cancelled_before_write"));
    assert_eq!(
        fixture
            .service
            .cancel_retained_agent_send(&cancelled)
            .unwrap()
            .error,
        receipt.error
    );
    writes.wait_for_flush();

    authenticate(&fixture, &tab);
    writes.wait_for_flush();
    assert_eq!(*writes.texts.lock(), ["first fragment", "second fragment"]);
    assert!(!fixture.has_pending_agent_delivery(&tab));
    for id in [&first, &second] {
        assert_eq!(
            fixture
                .service
                .inspect_retained_send(id)
                .unwrap()
                .unwrap()
                .state,
            ChatSendState::Unknown
        );
        assert!(fixture.service.cancel_retained_agent_send(id).is_err());
    }
    fixture.service.reconcile_authenticated_agent_input(&tab);
    writes.wait_for_flush();
    assert_eq!(*writes.texts.lock(), ["first fragment", "second fragment"]);
}

#[test]
fn never_ready_send_refuses_at_persisted_deadline_and_late_readiness_cannot_write_it() {
    use crate::cron::clock::MockClock;
    use crate::service::durable_delivery::DurableDeliveryRuntime;
    let (fixture, tab) = fixture(Some("saved-session"), false);
    let clock = MockClock::new();
    assert!(fixture
        .service
        .set_ext(DurableDeliveryRuntime::with_clock(clock.clone()))
        .is_ok());
    let writes = fixture.record_agent_deliveries(&tab);
    let id = admit(&fixture, &tab, "never ready fragment");
    let owner = &fixture.service.pending_delivery_payloads.agent_deliveries;
    let deadline = owner
        .pending_head(&tab)
        .unwrap()
        .unwrap()
        .ready_deadline_ms
        .unwrap();
    clock.set_ms(deadline - 1);
    fixture.service.wake_agent_deliveries_on_state_change(&tab);
    writes.wait_for_flush();
    assert_eq!(
        fixture
            .service
            .inspect_retained_send(&id)
            .unwrap()
            .unwrap()
            .state,
        ChatSendState::Pending
    );
    clock.set_ms(deadline);
    fixture.service.wake_agent_deliveries_on_state_change(&tab);
    writes.wait_for_flush();
    let receipt = fixture.service.inspect_retained_send(&id).unwrap().unwrap();
    assert_eq!(receipt.state, ChatSendState::Failed);
    assert_eq!(
        receipt.error.as_deref(),
        Some("initial_readiness_deadline_elapsed")
    );
    assert!(!fixture.has_pending_agent_delivery(&tab));
    assert!(fixture
        .ui
        .0
        .lock()
        .iter()
        .any(|(event, data)| event == "agent-delivery-status"
            && data["code"] == "initial_readiness_deadline_elapsed"
            && data["bytes_written"] == false));
    authenticate(&fixture, &tab);
    writes.wait_for_flush();
    assert!(writes.texts.lock().is_empty());
}
