use super::*;

#[test]
fn blueprint_v2_call_durable_review_first_authentication_binds_before_worker_observation() {
    let (fixture, tab, _) = restored();
    let writes = fixture.record_agent_deliveries(&tab);
    let id = admit(&fixture, &tab);
    writes.wait_for_flush();
    let owner = &fixture.service.pending_delivery_payloads.agent_deliveries;
    owner.state_changed(&tab);
    assert!(owner.start_worker(&tab), "test owns the paused worker");
    fixture.resume_recorded_agent_recipient(&tab, "saved-session", "L1");
    let first = owner.pending_head(&tab).unwrap().unwrap();
    assert_eq!(
        first
            .saved_launch
            .as_ref()
            .map(|launch| launch.nonce.as_str()),
        Some("L1")
    );
    fixture.resume_recorded_agent_recipient(&tab, "saved-session", "L2");
    owner.finish_worker(&tab);
    fixture.service.wake_agent_deliveries_on_state_change(&tab);
    writes.wait_for_flush();
    assert!(writes.texts.lock().is_empty());
    assert_failed(&fixture, &tab, &id, "launch_ended_before_write");
}

#[test]
fn blueprint_v2_call_durable_review_close_settles_recipient_gone() {
    let (fixture, tab, _) = restored();
    let writes = fixture.record_agent_deliveries(&tab);
    let id = admit(&fixture, &tab);
    writes.wait_for_flush();
    let workspace = fixture.service.mux.read().list_workspaces()[0].id.clone();
    fixture.service.mux.write().create_tab(&workspace).unwrap();
    fixture.service.close_tab(&tab).unwrap();
    assert!(!fixture.has_pending_agent_delivery(&tab));
    let events = fixture.ui.0.lock();
    assert!(events
        .iter()
        .any(|(kind, value)| kind == "agent-delivery-status"
            && value["delivery_id"] == id
            && value["code"] == "recipient_gone"));
    assert!(writes.texts.lock().is_empty());
}

#[test]
fn blueprint_v2_call_durable_review_agent_exit_keeps_the_true_cause() {
    let (fixture, tab, _) = restored();
    let writes = fixture.record_agent_deliveries(&tab);
    let id = admit(&fixture, &tab);
    writes.wait_for_flush();
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab)
        .unwrap()
        .agent_type = None;
    fixture.service.wake_agent_deliveries_on_state_change(&tab);
    writes.wait_for_flush();
    let events = fixture.ui.0.lock();
    let failure = events
        .iter()
        .find(|(kind, value)| {
            kind == "agent-delivery-status"
                && value["delivery_id"] == id
                && value["status"] == "failed"
        })
        .unwrap();
    assert_eq!(failure.1["code"], "recipient_agent_exited");
    assert!(writes.texts.lock().is_empty());
}

#[test]
fn blueprint_v2_call_durable_review_index_carries_deadline_and_target_without_payload_decode() {
    let (fixture, tab, _) = restored();
    let writes = fixture.record_agent_deliveries(&tab);
    let id = admit(&fixture, &tab);
    writes.wait_for_flush();
    let directory = fixture
        .service
        .agent_pad_data_dir
        .join("agent-delivery-outbox");
    let index = std::fs::read_dir(directory)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path()
        .join("index.json");
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(index).unwrap()).unwrap();
    let record = value["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["id"] == id)
        .unwrap();
    assert_eq!(record["durable_delivery"]["created_at_ms"], 1000);
    assert_eq!(record["durable_delivery"]["saved_target"]["tab"], tab);
}

#[test]
fn blueprint_v2_call_durable_review_restart_finds_an_orphaned_outbox() {
    let (fixture, tab, _) = restored();
    let writes = fixture.record_agent_deliveries(&tab);
    let id = admit(&fixture, &tab);
    writes.wait_for_flush();
    let data = fixture.service.agent_pad_data_dir.clone();
    let snapshot = fixture.service.mux.read().to_session_snapshot();
    drop(writes);
    // Crash snapshot excludes the recipient, without invoking graceful close settlement.
    let mut mux = MultiplexerState::from_session_snapshot(&snapshot);
    let workspace = mux.list_workspaces()[0].id.clone();
    mux.create_tab(&workspace).unwrap();
    let reservation = mux.reserve_tab_close(&tab).unwrap();
    mux.close_reserved_tab_with_reason(
        reservation,
        crate::multiplexer::types::TabCloseReason::User,
    )
    .unwrap();
    let restarted = ServiceFixture::new_at(mux, &data);
    let finished = restarted.record_agent_deliveries(&tab);
    let (event_tx, event_rx) = mpsc::channel();
    restarted.ui.listen("agent-delivery-status", move |event| {
        let _ = event_tx.send(event.clone());
    });
    restarted
        .service
        .schedule_restored_agent_delivery_reconciliation();
    let failure = event_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("startup settles orphaned durable authority");
    finished.wait_for_flush();
    assert_eq!(failure["delivery_id"], id);
    assert_eq!(failure["code"], "recipient_gone");
    assert!(!restarted.has_pending_agent_delivery(&tab));
}

#[test]
fn blueprint_v2_call_durable_review_expiry_is_checked_before_any_durable_claim() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    struct ExpiringClock {
        reads: AtomicUsize,
        saw_claimed: Arc<AtomicBool>,
        index: std::path::PathBuf,
    }
    impl crate::cron::clock::Clock for ExpiringClock {
        fn now_ms(&self) -> u64 {
            if self.reads.fetch_add(1, Ordering::SeqCst) == 0 {
                return 1000;
            }
            let value: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&self.index).unwrap()).unwrap();
            self.saw_claimed
                .store(value["records"][0]["state"] == "claimed", Ordering::SeqCst);
            1000 + DELIVERY_TTL_MS
        }
    }
    let (mux, tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let writes = fixture.bind_recorded_agent_recipient(&tab, "live-session", "live-launch");
    let owner = &fixture.service.pending_delivery_payloads.agent_deliveries;
    let mut payload = super::super::payload();
    payload.expected_pty_id = Some(format!("recorded-{tab}"));
    payload.expected_session_id = Some("live-session".into());
    payload.expected_binding_nonce = Some("live-launch".into());
    payload.durable_delivery = Some(crate::service::durable_delivery::DurableDelivery {
        producer: "original-run".into(),
        producer_tab: tab.clone(),
        created_at_ms: 1000,
        saved_target: None,
    });
    let id = owner
        .enqueue(&tab, &serde_json::to_string(&payload).unwrap(), None)
        .unwrap();
    let directory = fixture
        .service
        .agent_pad_data_dir
        .join("agent-delivery-outbox");
    let index = std::fs::read_dir(directory)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path()
        .join("index.json");
    let saw_claimed = Arc::new(AtomicBool::new(false));
    assert!(fixture
        .service
        .set_ext(DurableDeliveryRuntime::with_clock(Arc::new(
            ExpiringClock {
                reads: AtomicUsize::new(0),
                saw_claimed: saw_claimed.clone(),
                index,
            }
        )))
        .is_ok());
    let record = owner.pending_head(&tab).unwrap().unwrap();
    fixture.service.flush_agent_delivery_record(
        &tab,
        owner,
        &record,
        &payload,
        owner.state_generation(&tab),
    );
    assert!(
        !saw_claimed.load(Ordering::SeqCst),
        "TTL expiry must be observed while the record is still Pending"
    );
    assert!(writes.texts.lock().is_empty());
    assert_failed(&fixture, &tab, &id, "delivery_ttl_expired");
}

#[test]
fn blueprint_v2_call_durable_review_authentication_persistence_failure_never_allows_later_adoption()
{
    let (fixture, tab, _) = restored();
    let writes = fixture.record_agent_deliveries(&tab);
    let id = admit(&fixture, &tab);
    writes.wait_for_flush();
    let owner = &fixture.service.pending_delivery_payloads.agent_deliveries;
    owner.state_changed(&tab);
    assert!(owner.start_worker(&tab));
    let root = fixture
        .service
        .agent_pad_data_dir
        .join("agent-delivery-outbox");
    let blocked_write = std::fs::read_dir(root)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path()
        .join("index.next.json");
    std::fs::create_dir(&blocked_write).unwrap();
    crate::service::orchestrator::mark_tab_agent_launch_started(&fixture.service.mux, &tab);
    let _pty = fixture.bind_test_pty_session(&tab, "saved-session", "L1");
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab)
        .unwrap()
        .agent_booting = false;
    let authority = fixture.service.user_send_authority(&tab).unwrap();
    assert!(!fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .ready_launch_is_latched(&authority));
    fixture.service.reconcile_authenticated_agent_input(&tab);
    assert!(
        fixture
            .service
            .pending_delivery_payloads
            .user_sends
            .ready_launch_is_latched(&authority),
        "ordinary input reaches readiness despite durable binding persistence failure"
    );
    assert!(owner
        .pending_head(&tab)
        .unwrap()
        .unwrap()
        .saved_launch
        .is_none());
    std::fs::remove_dir(blocked_write).unwrap();
    fixture.resume_recorded_agent_recipient(&tab, "saved-session", "L2");
    owner.finish_worker(&tab);
    fixture.service.wake_agent_deliveries_on_state_change(&tab);
    writes.wait_for_flush();
    assert!(writes.texts.lock().is_empty());
    assert_failed(&fixture, &tab, &id, "launch_authority_unavailable");
}
