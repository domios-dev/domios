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

#[test]
fn blueprint_v2_call_durable_r3_restart_after_receipt_publication_recovers_status_once() {
    let (fixture, tab, id, clock) = held();
    let owner = &fixture.service.pending_delivery_payloads.agent_deliveries;
    owner
        .mark_terminal_failure(&tab, &id, "recipient_gone", None)
        .unwrap();
    let failure = serde_json::json!({
        "tab_id": tab, "delivery_id": id, "producer": "original-run",
        "producer_tab": tab, "created_at_ms": 1000,
        "status": "failed", "code": "recipient_gone", "detail": null,
        "bytes_written": false,
    });
    let body = serde_json::to_vec_pretty(&failure).unwrap();
    let (receipt, created) = fixture
        .service
        .ensure_agent_pad(&tab)
        .unwrap()
        .write_delivery_receipt(&id, &body)
        .unwrap();
    assert!(created);
    assert!(owner.claim_terminal_notification(&tab, &id).unwrap());
    assert!(!owner.claim_terminal_notification(&tab, &id).unwrap());
    let before_crash: Vec<_> = fixture
        .ui
        .0
        .lock()
        .iter()
        .filter(|(kind, event)| kind == "agent-delivery-status" && event["delivery_id"] == id)
        .map(|(_, event)| event.clone())
        .collect();
    assert_eq!(
        before_crash,
        [serde_json::json!({
            "tab_id": tab, "delivery_id": id, "status": "pending",
            "producer": "original-run", "created_at_ms": 1000, "bytes_written": false,
        })]
    );
    let next = restarted(&fixture, clock);
    let writes = next.record_agent_deliveries(&tab);
    let owner = &next.service.pending_delivery_payloads.agent_deliveries;
    next.service
        .expire_due_durable_deliveries(&tab, owner)
        .unwrap();
    next.service
        .expire_due_durable_deliveries(&tab, owner)
        .unwrap();
    let events = next.ui.0.lock();
    let failures: Vec<_> = events
        .iter()
        .filter(|(kind, event)| kind == "agent-delivery-status" && event["delivery_id"] == id)
        .map(|(_, event)| event.clone())
        .collect();
    assert_eq!(failures, [failure]);
    assert_eq!(
        before_crash
            .iter()
            .chain(failures.iter())
            .filter(|event| event["status"] == "failed")
            .count(),
        1
    );
    assert_eq!(
        next.service
            .notif
            .lock()
            .list(false)
            .iter()
            .filter(|notice| notice.body.as_deref().is_some_and(|body| body
                .contains("original-run")
                && body.contains("recipient_gone")))
            .count(),
        1
    );
    assert_eq!(std::fs::read(receipt).unwrap(), body);
    assert!(!next.has_pending_agent_delivery(&tab));
    assert!(writes.texts.lock().is_empty());
}

#[test]
fn blueprint_v2_call_durable_r3_edited_receipt_preserves_file_and_unblocks_later_result() {
    let (fixture, tab, id, _) = held();
    let writes = fixture.record_agent_deliveries(&tab);
    let owner = &fixture.service.pending_delivery_payloads.agent_deliveries;
    let record = owner.pending_head(&tab).unwrap().unwrap();
    let payload: AgentDeliveryPayload =
        serde_json::from_str(&owner.payload(&tab, &record).unwrap()).unwrap();
    let (receipt, _) = fixture
        .service
        .ensure_agent_pad(&tab)
        .unwrap()
        .write_delivery_receipt(&id, b"externally edited receipt")
        .unwrap();
    let mut envelope = MessageEnvelope::codeterm_reminder("blueprint:later-result");
    envelope.durable_delivery = Some(fixture.service.durable_delivery("later-run", &tab));
    let Delivery::Pending {
        delivery_id: later_id,
    } = Messenger::deliver(&fixture.service, &tab, envelope, "later body").unwrap()
    else {
        panic!("later durable admission");
    };
    assert!(
        !owner.start_worker(&tab),
        "the held head is parked until an authority state change"
    );
    assert_eq!(
        owner
            .records(&tab)
            .unwrap()
            .iter()
            .map(|record| record.id.as_str())
            .collect::<Vec<_>>(),
        [id.as_str(), later_id.as_str()]
    );
    assert!(writes.texts.lock().is_empty());
    fixture
        .service
        .fail_durable_delivery(&tab, owner, &record, &payload, "recipient_gone")
        .unwrap();
    assert_eq!(
        std::fs::read(receipt).unwrap(),
        b"externally edited receipt"
    );
    assert_eq!(owner.pending_head(&tab).unwrap().unwrap().id, later_id);
    fixture.resume_recorded_agent_recipient(&tab, "saved-session", "first-launch");
    writes.wait_for_flush();
    assert_eq!(
        writes.texts.lock().as_slice(),
        ["Original run: later-run; timestamp: 1000\nlater body"]
    );
    assert!(!fixture.has_pending_agent_delivery(&tab));
    assert!(fixture
        .ui
        .0
        .lock()
        .iter()
        .any(|(kind, event)| kind == "agent-delivery-status"
            && event["delivery_id"] == id
            && event["code"] == "recipient_gone"));
}

#[test]
fn blueprint_v2_call_durable_r2_concurrent_terminal_settlement_publishes_one_receipt_and_event() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };
    use std::time::Duration;
    let (fixture, tab, id, _) = held();
    let writes = fixture.record_agent_deliveries(&tab);
    let owner = fixture
        .service
        .pending_delivery_payloads
        .agent_deliveries
        .clone();
    let record = owner.pending_head(&tab).unwrap().unwrap();
    let payload: AgentDeliveryPayload =
        serde_json::from_str(&owner.payload(&tab, &record).unwrap()).unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let seen = count.clone();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = parking_lot::Mutex::new(release_rx);
    fixture.ui.listen("agent-delivery-status", move |event| {
        if event["status"] == "failed" && seen.fetch_add(1, Ordering::SeqCst) == 0 {
            entered_tx.send(()).unwrap();
            release_rx
                .lock()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
        }
    });
    let service = fixture.service.clone();
    let first_owner = owner.clone();
    let first_record = record.clone();
    let first_payload = payload.clone();
    let target = tab.clone();
    let closing = std::thread::spawn(move || {
        let _claim = service.pty_ownership.acquire(&target);
        service.fail_durable_delivery(
            &target,
            &first_owner,
            &first_record,
            &first_payload,
            "recipient_gone",
        )
    });
    let entered = entered_rx.recv_timeout(Duration::from_secs(5));
    if entered.is_ok() {
        fixture
            .service
            .fail_durable_delivery(&tab, &owner, &record, &payload, "delivery_ttl_expired")
            .unwrap();
        assert!(
            fixture.has_pending_agent_delivery(&tab),
            "the emit owner retains the head until it finishes"
        );
    }
    release_tx.send(()).unwrap();
    closing.join().unwrap().unwrap();
    entered.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert!(writes.texts.lock().is_empty());
    assert!(!fixture.has_pending_agent_delivery(&tab));
    let inbox = fixture
        .service
        .ensure_agent_pad(&tab)
        .unwrap()
        .path()
        .join("inbox");
    let files = std::fs::read_dir(inbox)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(
        files[0].file_name(),
        std::ffi::OsStr::new(&format!("delivery-{id}.json"))
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(files[0].path()).unwrap()).unwrap();
    assert_eq!(receipt["code"], "recipient_gone");
}

#[test]
fn blueprint_v2_call_durable_r2_binding_failure_does_not_poison_a_later_live_result() {
    let (fixture, tab, id, _) = held();
    let writes = fixture.record_agent_deliveries(&tab);
    let owner = &fixture.service.pending_delivery_payloads.agent_deliveries;
    owner.state_changed(&tab);
    assert!(owner.start_worker(&tab));
    fixture
        .service
        .remember_binding_failure(&tab, "injected durable persist failure");
    let mut ordinary = AgentDeliveryPayload {
        durable_delivery: None,
        trace_id: None,
        transport: Messenger::from(MessageEnvelope::from_codeterm())
            .framed_delivery("ordinary backlog"),
        observer: None,
        idempotency_key: None,
        expected_pty_id: Some(format!("recorded-{tab}")),
        expected_session_id: Some("saved-session".into()),
        expected_binding_nonce: Some("L1".into()),
    };
    owner
        .enqueue(&tab, &serde_json::to_string(&ordinary).unwrap(), None)
        .unwrap();
    fixture.resume_recorded_agent_recipient(&tab, "saved-session", "L1");
    let record = owner.pending_head(&tab).unwrap().unwrap();
    assert_eq!(record.id, id);
    let payload = serde_json::from_str(&owner.payload(&tab, &record).unwrap()).unwrap();
    fixture
        .service
        .fail_durable_delivery(
            &tab,
            owner,
            &record,
            &payload,
            "launch_authority_unavailable",
        )
        .unwrap();
    owner.finish_worker(&tab);
    fixture.service.wake_agent_deliveries_on_state_change(&tab);
    writes.wait_for_flush();
    assert_eq!(writes.texts.lock().as_slice(), ["ordinary backlog"]);
    ordinary.durable_delivery = Some(fixture.service.durable_delivery("new-live-run", &tab));
    ordinary.transport = ordinary.transport.with_body("new live result".into());
    owner
        .enqueue(&tab, &serde_json::to_string(&ordinary).unwrap(), None)
        .unwrap();
    fixture.service.wake_agent_deliveries_on_state_change(&tab);
    writes.wait_for_flush();
    assert_eq!(
        writes.texts.lock().as_slice(),
        ["ordinary backlog", "new live result"]
    );
    assert!(!fixture.has_pending_agent_delivery(&tab));
}

#[test]
fn blueprint_v2_call_durable_r2_held_prefix_is_inside_the_provider_paste_limit() {
    let (fixture, tab, _, _) = held();
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("saved-provider")
            .with_paste_limit_bytes(4096),
    ));
    let writes = fixture.record_agent_deliveries(&tab);
    let mut envelope = MessageEnvelope::codeterm_reminder("blueprint:limited-result");
    envelope.durable_delivery = Some(fixture.service.durable_delivery("limited-run", &tab));
    let messenger = Messenger::from(envelope.clone());
    let overhead = messenger.framed_delivery("").to_message().len();
    let body = "x".repeat(4096 - overhead - 1);
    assert!(messenger.framed_delivery(&body).to_message().len() < 4096);
    let Delivery::Pending { delivery_id } =
        Messenger::deliver(&fixture.service, &tab, envelope, &body).unwrap()
    else {
        panic!("restored limited result stays held");
    };
    fixture.service.wake_agent_deliveries_on_state_change(&tab);
    writes.wait_for_flush();
    let owner = &fixture.service.pending_delivery_payloads.agent_deliveries;
    let record = owner
        .records(&tab)
        .unwrap()
        .into_iter()
        .find(|record| record.id == delivery_id)
        .unwrap();
    let payload: AgentDeliveryPayload =
        serde_json::from_str(&owner.payload(&tab, &record).unwrap()).unwrap();
    assert!(
        payload.transport.to_message().len() <= 4096,
        "the actual queued frame respects the provider limit"
    );
    let pad = fixture.service.ensure_agent_pad(&tab).unwrap();
    let expected = format!("Original run: limited-run; timestamp: 1000\n{body}");
    assert!(pad
        .list()
        .unwrap()
        .iter()
        .any(|path| pad.read_utf8(path).is_ok_and(|text| text == expected)));
    assert!(writes.texts.lock().is_empty());
}

#[test]
fn blueprint_v2_call_durable_r2_refuses_when_even_the_held_pointer_exceeds_the_paste_limit() {
    let (fixture, tab, _, _) = held();
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("saved-provider").with_paste_limit_bytes(96),
    ));
    let writes = fixture.record_agent_deliveries(&tab);
    let owner = &fixture.service.pending_delivery_payloads.agent_deliveries;
    let accepted_before = owner.records(&tab).unwrap().len();
    let mut envelope = MessageEnvelope::codeterm_reminder("blueprint:limited-result");
    envelope.durable_delivery = Some(fixture.service.durable_delivery("limited-run", &tab));
    let error = Messenger::deliver(&fixture.service, &tab, envelope, &"x".repeat(512)).unwrap_err();
    assert!(error.contains("exceeds the provider paste limit"));
    assert_eq!(
        owner.records(&tab).unwrap().len(),
        accepted_before,
        "no oversized held payload is admitted"
    );
    assert!(writes.texts.lock().is_empty());
}

#[test]
fn blueprint_v2_call_durable_r2_unreadable_index_failure_excludes_new_saved_admissions() {
    let (fixture, tab, original_id, clock) = held();
    let next = restarted(&fixture, clock);
    let writes = next.record_agent_deliveries(&tab);
    let owner = &next.service.pending_delivery_payloads.agent_deliveries;
    owner.state_changed(&tab);
    assert!(owner.start_worker(&tab));
    let root = next
        .service
        .agent_pad_data_dir
        .join("agent-delivery-outbox");
    let index = std::fs::read_dir(root)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path()
        .join("index.json");
    let original_index = std::fs::read(&index).unwrap();
    std::fs::write(&index, "invalid outbox index").unwrap();
    next.resume_recorded_agent_recipient(&tab, "saved-session", "L1");
    std::fs::write(&index, original_index).unwrap();
    next.service
        .mux
        .write()
        .get_tab_mut(&tab)
        .unwrap()
        .restored_from_snapshot = true;
    let mut envelope = MessageEnvelope::codeterm_reminder("blueprint:new-result");
    envelope.durable_delivery = Some(next.service.durable_delivery("newer-run", &tab));
    let Delivery::Pending { delivery_id } =
        Messenger::deliver(&next.service, &tab, envelope, "new held result").unwrap()
    else {
        panic!("new saved result is admitted independently");
    };
    let records = owner.records(&tab).unwrap();
    let original = records
        .iter()
        .find(|record| record.id == original_id)
        .unwrap();
    let newer = records
        .iter()
        .find(|record| record.id == delivery_id)
        .unwrap();
    assert!(next.service.binding_failure(&tab, original).is_some());
    assert!(next.service.binding_failure(&tab, newer).is_none());
    next.resume_recorded_agent_recipient(&tab, "saved-session", "L2");
    owner.finish_worker(&tab);
    next.service.wake_agent_deliveries_on_state_change(&tab);
    writes.wait_for_flush();
    assert_eq!(
        writes.texts.lock().as_slice(),
        ["Original run: newer-run; timestamp: 1000\nnew held result"]
    );
    assert!(!next.has_pending_agent_delivery(&tab));
    assert!(next
        .ui
        .0
        .lock()
        .iter()
        .any(|(kind, event)| kind == "agent-delivery-status"
            && event["delivery_id"] == original_id
            && event["code"] == "launch_authority_unavailable"));
}
