use super::*;
use crate::cron::clock::MockClock;
use crate::multiplexer::state::MultiplexerState;
use crate::service::messenger::{Delivery, MessageEnvelope, Messenger};
use crate::service::test_fixture::ServiceFixture;
use std::sync::Arc;

fn held() -> (ServiceFixture, String, String, Arc<MockClock>) {
    let (mut mux, tab) = MultiplexerState::new();
    let value = mux.get_tab_mut(&tab).unwrap();
    value.agent_type = Some("saved-provider".into());
    value.agent_session_id = Some("saved-session".into());
    let fixture = ServiceFixture::new(MultiplexerState::from_session_snapshot(
        &mux.to_session_snapshot(),
    ));
    let clock = MockClock::starting_at(1000);
    assert!(fixture
        .service
        .set_ext(DurableDeliveryRuntime::with_clock(clock.clone()))
        .is_ok());
    let writes = fixture.record_agent_deliveries(&tab);
    let mut envelope = MessageEnvelope::codeterm_reminder("blueprint:result");
    envelope.durable_delivery = Some(fixture.service.durable_delivery("original-run", &tab));
    let Delivery::Pending { delivery_id } =
        Messenger::deliver(&fixture.service, &tab, envelope, "result").unwrap()
    else {
        panic!("durable admission");
    };
    writes.wait_for_flush();
    (fixture, tab, delivery_id, clock)
}

fn restarted(fixture: &ServiceFixture, clock: Arc<MockClock>) -> ServiceFixture {
    let mux =
        MultiplexerState::from_session_snapshot(&fixture.service.mux.read().to_session_snapshot());
    let next = ServiceFixture::new_at(mux, &fixture.service.agent_pad_data_dir);
    assert!(next
        .service
        .set_ext(DurableDeliveryRuntime::with_clock(clock))
        .is_ok());
    next
}

#[test]
fn blueprint_v2_call_durable_review_restart_unbound_survives_and_timer_is_restored_once() {
    let (fixture, tab, id, clock) = held();
    let next = restarted(&fixture, clock);
    let writes = next.record_agent_deliveries(&tab);
    next.service.restore_durable_expiries(&tab).unwrap();
    next.service.restore_durable_expiries(&tab).unwrap();
    assert_eq!(next.service.durable_runtime().timers.lock().len(), 1);
    let owner = &next.service.pending_delivery_payloads.agent_deliveries;
    assert_eq!(owner.pending_head(&tab).unwrap().unwrap().id, id);
    next.service
        .schedule_restored_agent_delivery_reconciliation();
    writes.wait_for_flush();
    assert!(writes.texts.lock().is_empty());
    next.resume_recorded_agent_recipient(&tab, "saved-session", "first-after-restart");
    writes.wait_for_flush();
    assert_eq!(
        writes.texts.lock().as_slice(),
        ["Original run: original-run; timestamp: 1000\nresult"]
    );
    assert!(!next.has_pending_agent_delivery(&tab));
    assert!(next.service.durable_runtime().timers.lock().is_empty());
}

#[test]
fn blueprint_v2_call_durable_review_restart_bound_but_unwritten_fails_without_replay() {
    let (fixture, tab, id, clock) = held();
    let owner = &fixture.service.pending_delivery_payloads.agent_deliveries;
    owner
        .bind_saved_launch(
            &tab,
            &id,
            SavedLaunch {
                pty: "old-pty".into(),
                session: "saved-session".into(),
                nonce: "old-launch".into(),
            },
        )
        .unwrap();
    let next = restarted(&fixture, clock);
    let writes = next.record_agent_deliveries(&tab);
    next.service
        .schedule_restored_agent_delivery_reconciliation();
    writes.wait_for_flush();
    assert!(!next.has_pending_agent_delivery(&tab));
    assert!(writes.texts.lock().is_empty());
    assert!(next
        .ui
        .0
        .lock()
        .iter()
        .any(|(kind, event)| kind == "agent-delivery-status"
            && event["delivery_id"] == id
            && event["code"] == "launch_ended_before_write"));
}

#[test]
fn blueprint_v2_call_durable_review_restart_previous_terminal_failure_retains_original_cause() {
    let (fixture, tab, id, clock) = held();
    let owner = &fixture.service.pending_delivery_payloads.agent_deliveries;
    owner
        .mark_terminal_failure(
            &tab,
            &id,
            "recipient_launch_failed",
            Some("saved launch failed before crash"),
        )
        .unwrap();
    let next = restarted(&fixture, clock);
    let writes = next.record_agent_deliveries(&tab);
    next.service
        .schedule_restored_agent_delivery_reconciliation();
    writes.wait_for_flush();
    assert!(!next.has_pending_agent_delivery(&tab));
    assert!(writes.texts.lock().is_empty());
    assert!(next
        .ui
        .0
        .lock()
        .iter()
        .any(|(kind, event)| kind == "agent-delivery-status"
            && event["delivery_id"] == id
            && event["code"] == "recipient_launch_failed"
            && event["detail"] == "saved launch failed before crash"));
}

#[test]
fn blueprint_v2_call_durable_review_live_recipient_keeps_body_and_uses_existing_writer_once() {
    let (mux, tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let writes = fixture.bind_recorded_agent_recipient(&tab, "live-session", "live-launch");
    let mut envelope = MessageEnvelope::codeterm_reminder("blueprint:live-result");
    envelope.durable_delivery = Some(fixture.service.durable_delivery("live-run", &tab));
    let delivery =
        Messenger::deliver(&fixture.service, &tab, envelope, "original live body").unwrap();
    assert!(matches!(delivery, Delivery::Pty { .. }));
    assert_eq!(writes.texts.lock().as_slice(), ["original live body"]);
    assert!(!fixture.has_pending_agent_delivery(&tab));
    assert!(fixture.service.durable_runtime().timers.lock().is_empty());
}

#[test]
fn blueprint_v2_call_durable_review_fired_timer_is_removed_before_failure_settlement() {
    let (mut mux, tab) = MultiplexerState::new();
    mux.get_tab_mut(&tab).unwrap().agent_type = Some("producer".into());
    let fixture = ServiceFixture::new(mux);
    let writes = fixture.record_agent_deliveries(&tab);
    let mut delivery = fixture.service.durable_delivery("original-run", &tab);
    delivery.created_at_ms = delivery.created_at_ms.saturating_sub(DELIVERY_TTL_MS);
    let payload = AgentDeliveryPayload {
        durable_delivery: Some(delivery),
        trace_id: None,
        transport: Messenger::from(MessageEnvelope::codeterm_reminder("blueprint:result"))
            .framed_delivery("expired result"),
        observer: None,
        idempotency_key: None,
        expected_pty_id: None,
        expected_session_id: None,
        expected_binding_nonce: None,
    };
    let owner = &fixture.service.pending_delivery_payloads.agent_deliveries;
    let id = owner
        .enqueue(&tab, &serde_json::to_string(&payload).unwrap(), None)
        .unwrap();
    fixture.service.schedule_durable_expiry(&tab, &id, &payload);
    writes.wait_for_flush();
    assert!(fixture.service.durable_runtime().timers.lock().is_empty());
    assert!(!fixture.has_pending_agent_delivery(&tab));
    assert!(writes.texts.lock().is_empty());
    assert!(fixture
        .ui
        .0
        .lock()
        .iter()
        .any(|(kind, event)| kind == "agent-delivery-status"
            && event["delivery_id"] == id
            && event["code"] == "delivery_ttl_expired"));
}

#[test]
fn blueprint_v2_call_durable_review_session_commit_binds_before_async_followup() {
    let (fixture, tab, id, _) = held();
    let writes = fixture.record_agent_deliveries(&tab);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_session("saved-provider", "saved-session"),
    ));
    let owner = &fixture.service.pending_delivery_payloads.agent_deliveries;
    owner.state_changed(&tab);
    assert!(owner.start_worker(&tab));
    let claim = fixture
        .service
        .commit_session_binding_custody(&tab, || {
            crate::service::orchestrator::mark_tab_agent_launch_started(&fixture.service.mux, &tab);
            {
                let mut mux = fixture.service.mux.write();
                mux.link_pty(&tab, "first-pty".into());
                let value = mux.get_tab_mut(&tab).unwrap();
                value.session_binding_nonce = Some("first-authenticated".into());
                value.reported_session_binding_nonce = None;
            }
            crate::service::hook_session_binding::bind_detected_session(
                &fixture.service.mux,
                fixture.service.agent_registry.as_ref(),
                &tab,
                "saved-provider",
                "saved-session",
                "first-authenticated",
            )
        })
        .unwrap();
    assert!(claim.nonce_verified);
    assert_eq!(
        owner
            .pending_head(&tab)
            .unwrap()
            .unwrap()
            .saved_launch
            .unwrap()
            .nonce,
        "first-authenticated"
    );
    // No binding follow-up or flush has run before the next launch authenticates.
    fixture.resume_recorded_agent_recipient(&tab, "saved-session", "second-authenticated");
    owner.finish_worker(&tab);
    fixture.service.wake_agent_deliveries_on_state_change(&tab);
    writes.wait_for_flush();
    assert!(writes.texts.lock().is_empty());
    assert!(!fixture.has_pending_agent_delivery(&tab));
    assert!(fixture
        .ui
        .0
        .lock()
        .iter()
        .any(|(kind, event)| kind == "agent-delivery-status"
            && event["delivery_id"] == id
            && event["code"] == "launch_ended_before_write"));
}
