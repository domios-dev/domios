use super::restored_launch_boundary;
use crate::multiplexer::state::MultiplexerState;
use crate::service::agent_delivery_outbox::test_boundary::Boundary;
use crate::service::delivery_error::DeliveryError;
use crate::service::messenger::{AgentDeliveryPayload, Delivery};
use crate::service::test_fixture::ServiceFixture;
use codeterm_types::pty::BootStage;
use parking_lot::Mutex;
use std::sync::{mpsc, Arc};
use std::time::Duration;

const PROVIDER: &str = "opaque-provider";
const SESSION: &str = "saved-session";
const SNAPSHOT_NONCE: &str = "snapshot-nonce";
const WAIT: Duration = Duration::from_secs(5);

type Writes = Arc<Mutex<Vec<(Option<String>, String)>>>;

fn restored_authenticated_tab() -> (ServiceFixture, String) {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some(PROVIDER.into());
    tab.agent_session_id = Some(SESSION.into());
    tab.pty_id = Some("snapshot-pty".into());
    tab.session_binding_nonce = Some(SNAPSHOT_NONCE.into());
    tab.reported_session_binding_nonce = Some(SNAPSHOT_NONCE.into());
    tab.restored_from_snapshot = true;
    tab.agent_booting = false;
    tab.protected = false;
    (ServiceFixture::new(mux), id)
}

fn payload() -> AgentDeliveryPayload {
    AgentDeliveryPayload {
        trace_id: None,
        transport: crate::service::messenger::Messenger::from(
            crate::service::messenger::MessageEnvelope::from_codeterm(),
        )
        .framed_delivery("opaque relaunch payload"),
        observer: None,
        idempotency_key: None,
        expected_pty_id: None,
        expected_session_id: None,
        expected_binding_nonce: None,
    }
}

/// Runs the materialization's synchronous launch steps: the restore claim, then the launch's
/// own nonce reservation.
fn install_relaunch(tab: &str, started: Arc<Mutex<Option<String>>>) {
    let tab = tab.to_string();
    restored_launch_boundary::install(Box::new(move |svc| {
        crate::service::pty_io::claim_restored_launch(
            &svc.mux,
            &tab,
            PROVIDER,
            Some(SESSION),
            None,
            true,
        )?
        .expect("restore claims its launch");
        let nonce =
            crate::service::orchestrator::reserve_launch_binding_nonce(svc, &tab, Some(SESSION));
        crate::service::orchestrator::mark_tab_agent_launch_started(&svc.mux, &tab);
        *started.lock() = nonce.clone();
        Ok(nonce)
    }));
}

fn install_writer(
    fixture: &ServiceFixture,
    tab: &str,
    result: fn() -> Result<(), String>,
) -> (Boundary, Writes, mpsc::Receiver<()>) {
    let writes = Writes::default();
    let captured = writes.clone();
    let mux = fixture.service.mux.clone();
    let owner = tab.to_string();
    let (done_tx, done_rx) = mpsc::channel();
    let boundary = Boundary::install(
        tab,
        Arc::new(move |payload| {
            let pty = mux
                .read()
                .get_tab(&owner)
                .and_then(|tab| tab.pty_id.clone());
            captured
                .lock()
                .push((pty, payload.transport.body().to_string()));
            result()
        }),
        Arc::new(move || done_tx.send(()).unwrap()),
    );
    (boundary, writes, done_rx)
}

fn deliver_pending(fixture: &ServiceFixture, tab: &str) -> String {
    match fixture
        .service
        .deliver_agent_payload(tab, payload(), "visible")
        .unwrap()
    {
        Delivery::Pending { delivery_id } => delivery_id,
        _ => panic!("a send to a relaunching tab is held"),
    }
}

fn register_relaunched_pty(fixture: &ServiceFixture, tab: &str) {
    let mut mux = fixture.service.mux.write();
    let tab = mux.get_tab_mut(tab).unwrap();
    tab.pty_id = Some("relaunched-pty".into());
    tab.agent_booting = false;
    tab.boot_stage = Some(BootStage::AgentReady);
}

fn bind_session(fixture: &ServiceFixture, tab: &str, session: &str) {
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(tab).unwrap();
        tab.agent_session_id = Some(session.into());
        tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    }
    fixture.service.reconcile_authenticated_agent_input(tab);
}

fn has_pending(fixture: &ServiceFixture, tab: &str) -> bool {
    fixture
        .service
        .pending_delivery_payloads
        .agent_deliveries
        .has_pending(tab)
        .unwrap()
}

fn assert_settles_failed_once(
    fixture: &ServiceFixture,
    tab: &str,
    end_launch: impl FnOnce(&ServiceFixture),
) {
    let started = Arc::default();
    install_relaunch(tab, Arc::clone(&started));
    let (_boundary, writes, done) = install_writer(fixture, tab, || Ok(()));
    let (failure_tx, failure_rx) = mpsc::channel();
    let listener = fixture.ui.listen("agent-delivery-status", move |event| {
        failure_tx.send(event.clone()).unwrap();
    });
    let delivery_id = deliver_pending(fixture, tab);
    end_launch(fixture);
    fixture.service.wake_agent_deliveries_on_state_change(tab);
    done.recv_timeout(WAIT).unwrap();
    assert_eq!(
        failure_rx.recv_timeout(WAIT).unwrap(),
        serde_json::json!({
            "tab_id": tab, "delivery_id": delivery_id, "status": "failed",
            "code": "launch_ended_before_write", "bytes_written": false,
        })
    );
    assert!(!has_pending(fixture, tab));
    fixture.service.wake_agent_deliveries_on_state_change(tab);
    done.recv_timeout(WAIT).unwrap();
    assert!(failure_rx.try_recv().is_err());
    assert!(writes.lock().is_empty());
    fixture.ui.unlisten(listener);
}

#[test]
fn restore_claim_voids_the_snapshot_proof_so_the_relaunch_reserves_its_nonce() {
    let (fixture, tab) = restored_authenticated_tab();
    crate::service::pty_io::claim_restored_launch(
        &fixture.service.mux,
        &tab,
        PROVIDER,
        Some(SESSION),
        None,
        true,
    )
    .unwrap()
    .expect("restore claims its launch");
    assert!(!fixture
        .service
        .mux
        .read()
        .get_tab(&tab)
        .unwrap()
        .launch_session_authenticated());
    let nonce = crate::service::orchestrator::reserve_launch_binding_nonce(
        &fixture.service,
        &tab,
        Some(SESSION),
    )
    .expect("an unauthenticated relaunch reserves on the caller's thread");
    assert_ne!(nonce, SNAPSHOT_NONCE);
}

#[test]
fn restored_send_drains_once_into_the_relaunched_pty_after_readiness() {
    let (fixture, tab) = restored_authenticated_tab();
    let started = Arc::default();
    install_relaunch(&tab, Arc::clone(&started));
    let (_boundary, writes, done) = install_writer(&fixture, &tab, || Ok(()));
    deliver_pending(&fixture, &tab);
    let relaunch_nonce = started.lock().clone().expect("relaunch nonce");
    assert_ne!(relaunch_nonce, SNAPSHOT_NONCE);
    assert!(writes.lock().is_empty());

    register_relaunched_pty(&fixture, &tab);
    fixture.service.wake_agent_deliveries_on_state_change(&tab);
    done.recv_timeout(WAIT).unwrap();
    assert!(
        writes.lock().is_empty(),
        "nothing is written before readiness"
    );
    assert!(has_pending(&fixture, &tab));

    bind_session(&fixture, &tab, SESSION);
    done.recv_timeout(WAIT).unwrap();
    assert_eq!(
        *writes.lock(),
        [(
            Some("relaunched-pty".to_string()),
            "opaque relaunch payload".to_string()
        )]
    );
    assert!(!has_pending(&fixture, &tab));
    fixture.service.reconcile_authenticated_agent_input(&tab);
    done.recv_timeout(WAIT).unwrap();
    assert_eq!(writes.lock().len(), 1);
}

#[test]
fn relaunch_bound_to_another_session_settles_failed_without_writing() {
    let (fixture, tab) = restored_authenticated_tab();
    assert_settles_failed_once(&fixture, &tab, |fixture| {
        register_relaunched_pty(fixture, &tab);
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab).unwrap();
        tab.agent_session_id = Some("foreign-session".into());
        tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    });
}

#[test]
fn provider_switch_after_relaunch_settles_failed_without_writing() {
    let (fixture, tab) = restored_authenticated_tab();
    assert_settles_failed_once(&fixture, &tab, |fixture| {
        fixture
            .service
            .reserve_agent_launch_binding_nonce(&tab, None);
        register_relaunched_pty(fixture, &tab);
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab).unwrap();
        tab.agent_type = Some("other-provider".into());
        tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    });
}

#[test]
fn failed_relaunch_settles_failed_without_writing() {
    let (fixture, tab) = restored_authenticated_tab();
    assert_settles_failed_once(&fixture, &tab, |fixture| {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab).unwrap();
        tab.agent_booting = false;
        tab.boot_stage = Some(BootStage::SpawnFailed);
    });
}

#[test]
fn refused_or_superseded_relaunch_refuses_the_send_before_admission() {
    for outcome in ["refused", "not_started", "superseded"] {
        let (fixture, tab) = restored_authenticated_tab();
        let owner = tab.clone();
        restored_launch_boundary::install(Box::new(move |svc| match outcome {
            "refused" => Err("restored_session_unavailable: opaque".into()),
            "not_started" => Ok(None),
            _ => {
                let nonce = svc.reserve_agent_launch_binding_nonce(&owner, Some(SESSION));
                svc.reserve_agent_launch_binding_nonce(&owner, Some(SESSION));
                Ok(Some(nonce))
            }
        }));
        let (_boundary, writes, _done) = install_writer(&fixture, &tab, || Ok(()));
        let error = fixture
            .service
            .deliver_agent_payload(&tab, payload(), "visible")
            .unwrap_err();
        assert!(
            outcome == "refused" || error.starts_with("agent_unavailable:"),
            "{outcome}: {error}"
        );
        assert!(!has_pending(&fixture, &tab));
        assert!(writes.lock().is_empty());
    }
}

#[test]
fn uncertain_relaunch_write_never_replays() {
    let (fixture, tab) = restored_authenticated_tab();
    let started = Arc::default();
    install_relaunch(&tab, Arc::clone(&started));
    let (_boundary, writes, done) = install_writer(&fixture, &tab, || {
        Err(DeliveryError::SubmitUnconfirmed {
            tab: "opaque".into(),
            detail: "opaque".into(),
        }
        .ledger_message())
    });
    deliver_pending(&fixture, &tab);
    register_relaunched_pty(&fixture, &tab);
    bind_session(&fixture, &tab, SESSION);
    done.recv_timeout(WAIT).unwrap();
    assert_eq!(writes.lock().len(), 1);
    assert!(!has_pending(&fixture, &tab));
    fixture.service.wake_agent_deliveries_on_state_change(&tab);
    done.recv_timeout(WAIT).unwrap();
    fixture.service.reconcile_authenticated_agent_input(&tab);
    done.recv_timeout(WAIT).unwrap();
    assert_eq!(writes.lock().len(), 1);
}
