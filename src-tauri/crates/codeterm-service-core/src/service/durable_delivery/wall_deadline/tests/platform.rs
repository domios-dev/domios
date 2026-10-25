use crate::cron::clock::{Clock, SystemClock};
use crate::multiplexer::state::MultiplexerState;
use crate::service::durable_delivery::DELIVERY_TTL_MS;
use crate::service::messenger::{AgentDeliveryPayload, MessageEnvelope, Messenger};
use crate::service::test_fixture::ServiceFixture;
use std::sync::mpsc;
use std::time::Duration;

fn assert_expired_deadline_fires_once<T>(
    arm: impl FnOnce(u64, Box<dyn FnOnce() + Send>) -> Result<T, String>,
) {
    let (tx, rx) = mpsc::channel();
    let timer = arm(
        SystemClock.now_ms().saturating_sub(1),
        Box::new(move || {
            let _ = tx.send(());
        }),
    )
    .unwrap();
    rx.recv_timeout(Duration::from_secs(5))
        .expect("backend absolute deadline fires without polling");
    drop(timer);
    assert!(rx.try_recv().is_err());
}

fn assert_fired_timer_removed_before_failure_settlement() {
    let (mut mux, tab) = MultiplexerState::new();
    mux.get_tab_mut(&tab).unwrap().agent_type = Some("producer".into());
    let fixture = ServiceFixture::new(mux);
    let writes = fixture.record_agent_deliveries(&tab);
    let mut delivery = fixture.service.durable_delivery("timer-backend-run", &tab);
    delivery.created_at_ms = delivery.created_at_ms.saturating_sub(DELIVERY_TTL_MS);
    let payload = AgentDeliveryPayload {
        durable_delivery: Some(delivery),
        trace_id: None,
        transport: Messenger::from(MessageEnvelope::codeterm_reminder("blueprint:result"))
            .framed_delivery("expired backend result"),
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
    let (settlement_tx, settlement_rx) = mpsc::channel();
    let service = fixture.service.weak_self();
    let target = tab.clone();
    let delivery_id = id.clone();
    fixture.ui.listen("agent-delivery-status", move |event| {
        if event["delivery_id"] == delivery_id && event["status"] == "failed" {
            let service = service.upgrade().unwrap();
            let timer_present = service
                .durable_runtime()
                .timers
                .lock()
                .contains_key(&(target.clone(), delivery_id.clone()));
            let terminal_record_present = service
                .pending_delivery_payloads
                .agent_deliveries
                .records(&target)
                .unwrap()
                .iter()
                .any(|record| record.id == delivery_id && record.terminal_failure.is_some());
            settlement_tx
                .send((timer_present, terminal_record_present, event.clone()))
                .unwrap();
        }
    });
    fixture.service.schedule_durable_expiry(&tab, &id, &payload);
    writes.wait_for_flush();
    let (timer_present, terminal_record_present, event) = settlement_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("native backend wakes failure settlement");
    assert!(!timer_present, "fired timer is absent during settlement");
    assert!(
        terminal_record_present,
        "settlement precedes record discard"
    );
    assert_eq!(event["code"], "delivery_ttl_expired");
    assert_eq!(event["bytes_written"], false);
    assert_eq!(event["producer"], "timer-backend-run");
    assert!(settlement_rx.try_recv().is_err());
    assert!(fixture.service.durable_runtime().timers.lock().is_empty());
    assert!(!fixture.has_pending_agent_delivery(&tab));
    assert!(writes.texts.lock().is_empty());
}

#[cfg(target_os = "macos")]
mod macos {
    use super::super::super::macos::WallDeadline;

    #[test]
    fn blueprint_v2_call_durable_macos_dispatch_walltime_expired_deadline_fires_once() {
        super::assert_expired_deadline_fires_once(WallDeadline::arm);
    }

    #[test]
    fn blueprint_v2_call_durable_macos_dispatch_walltime_fired_timer_removed_before_failure_settlement(
    ) {
        super::assert_fired_timer_removed_before_failure_settlement();
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::super::super::linux::WallDeadline;

    #[test]
    fn blueprint_v2_call_durable_linux_timerfd_expired_deadline_fires_once() {
        super::assert_expired_deadline_fires_once(WallDeadline::arm);
    }

    #[test]
    fn blueprint_v2_call_durable_linux_timerfd_fired_timer_removed_before_failure_settlement() {
        super::assert_fired_timer_removed_before_failure_settlement();
    }
}
