use super::*;
use crate::service::durable_delivery::{DurableDeliveryRuntime, DELIVERY_TTL_MS};
use crate::service::messenger::{MessageEnvelope, Messenger};

pub(super) fn restored() -> (ServiceFixture, String, Arc<crate::cron::clock::MockClock>) {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("saved-provider".into());
    tab.agent_session_id = Some("saved-session".into());
    tab.session_binding_nonce = Some("saved-nonce".into());
    tab.reported_session_binding_nonce = Some("saved-nonce".into());
    let mux = MultiplexerState::from_session_snapshot(&mux.to_session_snapshot());
    let fixture = ServiceFixture::new(mux);
    let clock = crate::cron::clock::MockClock::starting_at(1000);
    assert!(fixture
        .service
        .set_ext(DurableDeliveryRuntime::with_clock(clock.clone()))
        .is_ok());
    (fixture, id, clock)
}

pub(super) fn admit(fixture: &ServiceFixture, tab: &str) -> String {
    let mut envelope = MessageEnvelope::codeterm_reminder("blueprint:restored-result");
    envelope.durable_delivery = Some(fixture.service.durable_delivery("original-run", tab));
    match Messenger::deliver(&fixture.service, tab, envelope, "Read result.json").unwrap() {
        Delivery::Pending { delivery_id } => delivery_id,
        other => panic!("restored result must be held: {other:?}"),
    }
}

pub(super) fn assert_failed(fixture: &ServiceFixture, tab: &str, id: &str, reason: &str) {
    assert!(!fixture.has_pending_agent_delivery(tab));
    let event = fixture
        .ui
        .0
        .lock()
        .iter()
        .find_map(|(kind, value)| {
            (kind == "agent-delivery-status"
                && value["delivery_id"] == id
                && value["status"] == "failed")
                .then(|| value.clone())
        })
        .expect("producer receives a visible terminal event");
    assert_eq!(event["code"], reason);
    assert_eq!(event["producer"], "original-run");
    assert_eq!(event["created_at_ms"], 1000);
    assert_eq!(event["bytes_written"], false);
    let inbox = fixture
        .service
        .ensure_agent_pad(tab)
        .unwrap()
        .path()
        .join("inbox");
    let receipts = std::fs::read_dir(inbox)
        .unwrap()
        .map(|entry| {
            serde_json::from_str::<serde_json::Value>(
                &std::fs::read_to_string(entry.unwrap().path()).unwrap(),
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0], event);
    assert!(
        fixture
            .service
            .notif
            .lock()
            .list(false)
            .iter()
            .any(|notice| {
                notice
                    .body
                    .as_deref()
                    .is_some_and(|body| body.contains("original-run") && body.contains(reason))
            }),
        "live producer receives the existing failure notification"
    );
}

#[test]
fn blueprint_v2_call_durable_restored_result_is_admitted_to_disk_without_clearing_restore() {
    let (fixture, tab, _) = restored();
    let writes = fixture.record_agent_deliveries(&tab);
    let id = admit(&fixture, &tab);
    writes.wait_for_flush();
    assert!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&tab)
            .unwrap()
            .restored_from_snapshot
    );
    assert!(writes.texts.lock().is_empty());
    let owner = &fixture.service.pending_delivery_payloads.agent_deliveries;
    let record = owner.pending_head(&tab).unwrap().unwrap();
    assert_eq!(record.id, id);
    let persisted: AgentDeliveryPayload =
        serde_json::from_str(&owner.payload(&tab, &record).unwrap()).unwrap();
    let durable = persisted.durable_delivery.unwrap();
    assert_eq!(durable.producer, "original-run");
    assert_eq!(durable.created_at_ms, 1000);
    assert!(durable.saved_target.is_some());
}

#[test]
fn blueprint_v2_call_durable_matching_saved_session_drains_once_and_second_rebind_does_not_replay()
{
    let (fixture, tab, _) = restored();
    let writes = fixture.record_agent_deliveries(&tab);
    admit(&fixture, &tab);
    writes.wait_for_flush();
    fixture.resume_recorded_agent_recipient(&tab, "saved-session", "first-authenticated-launch");
    writes.wait_for_flush();
    assert_eq!(
        writes.texts.lock().as_slice(),
        ["Original run: original-run; timestamp: 1000\nRead result.json"]
    );
    assert!(!fixture.has_pending_agent_delivery(&tab));
    fixture.resume_recorded_agent_recipient(&tab, "saved-session", "second-launch");
    writes.wait_for_flush();
    assert_eq!(writes.texts.lock().len(), 1);
}

#[test]
fn blueprint_v2_call_durable_wrong_session_provider_or_failed_launch_settles_visibly() {
    for (case, reason) in [
        ("session", "saved_session_changed"),
        ("provider", "saved_provider_changed"),
        ("failed", "recipient_launch_failed"),
    ] {
        let (fixture, tab, _) = restored();
        let writes = fixture.record_agent_deliveries(&tab);
        let id = admit(&fixture, &tab);
        writes.wait_for_flush();
        match case {
            "session" => {
                fixture.resume_recorded_agent_recipient(&tab, "foreign-session", "fresh-nonce")
            }
            "provider" => {
                fixture
                    .service
                    .mux
                    .write()
                    .get_tab_mut(&tab)
                    .unwrap()
                    .agent_type = Some("foreign-provider".into());
                fixture.resume_recorded_agent_recipient(&tab, "saved-session", "fresh-nonce");
            }
            _ => {
                fixture
                    .service
                    .mux
                    .write()
                    .get_tab_mut(&tab)
                    .unwrap()
                    .boot_stage = Some(BootStage::SpawnFailed);
                fixture.service.wake_agent_deliveries_on_state_change(&tab);
            }
        }
        writes.wait_for_flush();
        assert!(writes.texts.lock().is_empty());
        assert_failed(&fixture, &tab, &id, reason);
    }
}

#[test]
fn blueprint_v2_call_durable_twenty_four_hour_expiry_uses_injected_clock_and_fails_visibly() {
    let (fixture, tab, clock) = restored();
    let writes = fixture.record_agent_deliveries(&tab);
    let id = admit(&fixture, &tab);
    writes.wait_for_flush();
    clock.advance_ms(DELIVERY_TTL_MS - 1);
    fixture.service.wake_agent_deliveries_on_state_change(&tab);
    writes.wait_for_flush();
    assert!(fixture.has_pending_agent_delivery(&tab));
    clock.advance_ms(1);
    fixture.service.wake_agent_deliveries_on_state_change(&tab);
    writes.wait_for_flush();
    assert!(writes.texts.lock().is_empty());
    assert_failed(&fixture, &tab, &id, "delivery_ttl_expired");
}

#[test]
fn blueprint_v2_call_durable_missing_producer_retains_failure_without_forging_agent_provenance() {
    let (fixture, tab, clock) = restored();
    let writes = fixture.record_agent_deliveries(&tab);
    let mut envelope = MessageEnvelope::codeterm_reminder("blueprint:restored-result");
    envelope.durable_delivery = Some(
        fixture
            .service
            .durable_delivery("original-run", "gone-producer"),
    );
    assert!(matches!(
        Messenger::deliver(&fixture.service, &tab, envelope, "retained result").unwrap(),
        Delivery::Pending { .. }
    ));
    writes.wait_for_flush();
    clock.advance_ms(DELIVERY_TTL_MS);
    fixture.service.wake_agent_deliveries_on_state_change(&tab);
    writes.wait_for_flush();
    assert!(writes.texts.lock().is_empty());
    assert!(!fixture.has_pending_agent_delivery(&tab));
    assert!(fixture.service.notif.lock().list(false).is_empty());
    let inbox = fixture
        .service
        .ensure_agent_pad("gone-producer")
        .unwrap()
        .path()
        .join("inbox");
    let files = std::fs::read_dir(inbox)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(files.len(), 1);
    let receipt: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(files[0].path()).unwrap()).unwrap();
    assert_eq!(receipt["producer_tab"], "gone-producer");
    assert_eq!(receipt["producer"], "original-run");
    assert_eq!(receipt["status"], "failed");
    assert_eq!(receipt["code"], "delivery_ttl_expired");
}

#[test]
fn blueprint_v2_call_durable_write_uncertainty_is_never_replayed_on_a_second_binding() {
    let (fixture, tab, _) = restored();
    let writes = Arc::new(Mutex::new(Vec::new()));
    let captured = writes.clone();
    let (done_tx, done_rx) = mpsc::channel();
    let _boundary = super::super::test_boundary::Boundary::install(
        &tab,
        Arc::new(move |payload| {
            captured.lock().push(payload.transport.body().to_string());
            Err("submit_unconfirmed: bytes written; exact turn evidence unavailable".into())
        }),
        Arc::new(move || {
            let _ = done_tx.send(());
        }),
    );
    admit(&fixture, &tab);
    done_rx.recv_timeout(Duration::from_secs(30)).unwrap();
    fixture.resume_recorded_agent_recipient(&tab, "saved-session", "first-launch");
    done_rx.recv_timeout(Duration::from_secs(30)).unwrap();
    assert!(!fixture.has_pending_agent_delivery(&tab));
    assert_eq!(writes.lock().len(), 1);
    fixture.resume_recorded_agent_recipient(&tab, "saved-session", "second-launch");
    done_rx.recv_timeout(Duration::from_secs(30)).unwrap();
    assert_eq!(writes.lock().len(), 1);
}

#[test]
fn blueprint_v2_call_durable_first_authenticated_launch_cannot_be_replaced_after_zero_write() {
    let (fixture, tab, _) = restored();
    let (done_tx, done_rx) = mpsc::channel();
    let _boundary = super::super::test_boundary::Boundary::install(
        &tab,
        Arc::new(|_| Err("tab_busy_refused: pre-write fixture refusal".into())),
        Arc::new(move || {
            let _ = done_tx.send(());
        }),
    );
    let id = admit(&fixture, &tab);
    done_rx.recv_timeout(Duration::from_secs(30)).unwrap();
    fixture.resume_recorded_agent_recipient(&tab, "saved-session", "first-launch");
    done_rx.recv_timeout(Duration::from_secs(30)).unwrap();
    let owner = &fixture.service.pending_delivery_payloads.agent_deliveries;
    assert_eq!(
        owner
            .pending_head(&tab)
            .unwrap()
            .unwrap()
            .saved_launch
            .unwrap()
            .nonce,
        "first-launch"
    );
    fixture.resume_recorded_agent_recipient(&tab, "saved-session", "second-launch");
    done_rx.recv_timeout(Duration::from_secs(30)).unwrap();
    assert_failed(&fixture, &tab, &id, "launch_ended_before_write");
}

#[path = "durable/review.rs"]
mod review;
