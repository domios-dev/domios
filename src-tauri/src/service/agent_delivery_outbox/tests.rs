use super::{
    retired_delivery_flags, retired_delivery_status, worker_brief_flagged,
    AgentDeliveryReceiptStatus,
};

use crate::multiplexer::state::MultiplexerState;
use crate::service::messenger::{AgentDeliveryPayload, Delivery, QueuedAgentPayload};
use crate::service::test_fixture::ServiceFixture;
use codeterm_types::pty::BootStage;
use parking_lot::Mutex;
use std::sync::{mpsc, Arc};
use std::time::Duration;

fn fixture(session: Option<&str>, prelaunch: bool) -> (ServiceFixture, String) {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("opaque-provider".into());
    tab.pty_id = (!prelaunch).then(|| "own-pty".into());
    tab.session_binding_nonce = Some("reserved-launch-nonce".into());
    tab.agent_session_id = session.map(str::to_string);
    tab.restored_from_snapshot = session.is_some();
    tab.agent_booting = prelaunch;
    tab.boot_stage = Some(if prelaunch {
        BootStage::PtyStarting
    } else {
        BootStage::AgentReady
    });
    let fixture = ServiceFixture::new(mux);
    if session.is_some() {
        crate::service::orchestrator::mark_tab_agent_launch_started(&fixture.service.mux, &id);
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.agent_booting = false;
        tab.boot_stage = Some(BootStage::AgentReady);
    }
    (fixture, id)
}

fn payload() -> AgentDeliveryPayload {
    AgentDeliveryPayload {
        trace_id: None,
        transport: QueuedAgentPayload::Text {
            text: "opaque held payload".into(),
        },
        observer: None,
        idempotency_key: None,
        expected_pty_id: None,
        expected_session_id: None,
        expected_binding_nonce: None,
    }
}

fn authenticate(fixture: &ServiceFixture, tab: &str) {
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(tab).unwrap();
        tab.pty_id = Some("own-pty".into());
        tab.agent_session_id.get_or_insert("fresh-session".into());
        tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
        tab.agent_booting = false;
        tab.boot_stage = Some(BootStage::AgentReady);
    }
    fixture.service.reconcile_authenticated_agent_input(tab);
}

fn assert_auth_drains_once(session: Option<&str>, prelaunch: bool) {
    let (fixture, tab) = fixture(session, prelaunch);
    assert_fixture_auth_drains_once(fixture, tab);
}

fn assert_fixture_auth_drains_once(fixture: ServiceFixture, tab: String) {
    let writes = Arc::new(Mutex::new(Vec::new()));
    let captured = writes.clone();
    let (done_tx, done_rx) = mpsc::channel();
    let _boundary = super::test_boundary::Boundary::install(
        &tab,
        Arc::new(move |payload| {
            let QueuedAgentPayload::Text { text } = &payload.transport else {
                panic!("expected text")
            };
            captured.lock().push(text.clone());
            Ok(())
        }),
        Arc::new(move || {
            done_tx.send(()).unwrap();
        }),
    );
    assert!(matches!(
        fixture
            .service
            .deliver_agent_payload(&tab, payload(), "visible")
            .unwrap(),
        Delivery::Pending { .. }
    ));
    assert!(writes.lock().is_empty());
    let resumed = fixture
        .service
        .mux
        .read()
        .get_tab(&tab)
        .unwrap()
        .agent_session_id
        .is_some();
    if resumed {
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&tab).unwrap();
            tab.pty_id = Some("own-pty".into());
            tab.boot_stage = Some(BootStage::AgentReady);
            tab.agent_booting = false;
        }
        fixture.service.wake_agent_deliveries_on_state_change(&tab);
        done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(writes.lock().is_empty());
        assert!(fixture
            .service
            .pending_delivery_payloads
            .agent_deliveries
            .has_pending(&tab)
            .unwrap());
    }
    authenticate(&fixture, &tab);
    done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(*writes.lock(), ["opaque held payload"]);
    assert!(!fixture
        .service
        .pending_delivery_payloads
        .agent_deliveries
        .has_pending(&tab)
        .unwrap());
    fixture.service.reconcile_authenticated_agent_input(&tab);
    done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(*writes.lock(), ["opaque held payload"]);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop ServiceContext fixture")]
fn immediate_restore_send_delivers_once_after_authentication() {
    let (fixture, tab) = fixture(Some("saved-session"), false);
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab).unwrap();
        tab.restored_from_snapshot = true;
        tab.agent_booting = false;
        tab.pty_id = Some("outgoing-pty".into());
    }
    crate::service::pty_io::claim_restored_launch(
        &fixture.service.mux,
        &tab,
        "opaque-provider",
        Some("saved-session"),
        None,
        true,
    )
    .unwrap()
    .expect("restore must claim its launch");
    fixture
        .service
        .reserve_agent_launch_binding_nonce(&tab, Some("saved-session"));
    assert_fixture_auth_drains_once(fixture, tab);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop ServiceContext fixture")]
fn immediate_relaunch_send_delivers_once_after_authentication() {
    let (fixture, tab) = fixture(Some("saved-session"), false);
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab).unwrap();
        tab.boot_stage = Some(BootStage::SpawnFailed);
        tab.pty_id = Some("outgoing-pty".into());
    }
    fixture
        .service
        .reserve_agent_launch_binding_nonce(&tab, Some("saved-session"));
    assert_fixture_auth_drains_once(fixture, tab);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop ServiceContext fixture")]
fn restored_send_before_auth_flushes_once_without_another_ready_edge() {
    assert_auth_drains_once(Some("saved-session"), false);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop ServiceContext fixture")]
fn fresh_send_before_launch_registration_queues_and_flushes_once() {
    assert_auth_drains_once(None, true);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop ServiceContext fixture")]
fn stale_pty_pin_emits_honest_zero_write_failure() {
    assert_ended_launch_fails_once(false);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop ServiceContext fixture")]
fn failed_relaunch_fails_held_send_once_without_writing() {
    assert_ended_launch_fails_once(true);
}

fn assert_ended_launch_fails_once(failed_launch: bool) {
    use tauri::Listener;
    let (fixture, tab) = fixture(Some("saved-session"), false);
    if failed_launch {
        fixture
            .service
            .reserve_agent_launch_binding_nonce(&tab, Some("saved-session"));
    }
    let (failure_tx, failure_rx) = mpsc::channel();
    let listener = fixture._app.listen("agent-delivery-status", move |event| {
        failure_tx
            .send(serde_json::from_str::<serde_json::Value>(event.payload()).unwrap())
            .unwrap();
    });
    let (done_tx, done_rx) = mpsc::channel();
    let _boundary = super::test_boundary::Boundary::install(
        &tab,
        Arc::new(|_| panic!("a stale payload must never be written")),
        Arc::new(move || {
            done_tx.send(()).unwrap();
        }),
    );
    let Delivery::Pending { delivery_id } = fixture
        .service
        .deliver_agent_payload(&tab, payload(), "visible")
        .unwrap()
    else {
        panic!("expected held send")
    };
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab).unwrap();
        if failed_launch {
            tab.agent_booting = false;
            tab.boot_stage = Some(BootStage::SpawnFailed);
        } else {
            tab.pty_id = Some("different-pty".into());
        }
    }
    fixture.service.wake_agent_deliveries_on_state_change(&tab);
    done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(
        failure_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        serde_json::json!({
            "tab_id": tab, "delivery_id": delivery_id, "status": "failed",
            "code": "launch_ended_before_write", "bytes_written": false,
        })
    );
    assert!(!fixture
        .service
        .pending_delivery_payloads
        .agent_deliveries
        .has_pending(&tab)
        .unwrap());
    fixture.service.wake_agent_deliveries_on_state_change(&tab);
    done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(failure_rx.try_recv().is_err());
    fixture._app.unlisten(listener);
}

#[test]
fn exact_turn_proof_retires_as_delivered() {
    assert_eq!(
        retired_delivery_status(Some(true)),
        AgentDeliveryReceiptStatus::Delivered
    );
}

#[test]
fn absent_turn_retires_as_submit_unconfirmed() {
    assert_eq!(
        retired_delivery_status(Some(false)),
        AgentDeliveryReceiptStatus::SubmitUnconfirmed
    );
}

#[test]
fn unknown_session_retires_as_submit_unconfirmed() {
    assert_eq!(
        retired_delivery_status(None),
        AgentDeliveryReceiptStatus::SubmitUnconfirmed
    );
}

#[test]
fn worker_brief_and_report_receipts_are_flagged() {
    assert!(worker_brief_flagged(Some(
        crate::multiplexer::types::AgentRole::Worker
    )));
    assert_eq!(retired_delivery_flags(true, true), (true, true));
    assert_eq!(retired_delivery_flags(false, true), (false, true));
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop ServiceContext fixture")]
fn live_resume_provisional_launch_delivers_held_payload_once_after_binding() {
    let (fixture, tab_id) = fixture(Some("saved-session"), false);
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.boot_stage = Some(BootStage::LaunchWritten);
        tab.provisional_launch = crate::service::boot_stage::provisional_launch_for(
            tab,
            "own-pty",
            "reserved-launch-nonce",
            "opaque-reason",
        );
        tab.boot_stage = Some(BootStage::LaunchBindingUnconfirmed);
    }
    assert_fixture_auth_drains_once(fixture, tab_id);
}
