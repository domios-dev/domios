use super::*;
use crate::agent::chat_types::ChatMsg;
use crate::multiplexer::state::MultiplexerState;
use crate::service::boot_stage::BootStage;
use crate::service::test_fixture::ServiceFixture;
use std::sync::{Arc, mpsc};

fn launch_fixture() -> (ServiceFixture, String, LaunchAttempt) {
    let (mut mux, parent_tab_id) = MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&parent_tab_id).unwrap();
    let tab_id = mux.create_tab(&workspace_id).unwrap();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("readiness-test-provider".into());
    tab.pty_id = Some("launch-pty".into());
    tab.agent_session_id = Some("launch-session".into());
    tab.session_binding_nonce = Some("launch-nonce".into());
    tab.reported_session_binding_nonce = Some("launch-nonce".into());
    tab.boot_stage = Some(BootStage::ProviderReady);
    let attempt = LaunchAttempt {
        tab_id: tab_id.clone(),
        pty_id: "launch-pty".into(),
        binding_nonce: "launch-nonce".into(),
    };
    (ServiceFixture::new(mux), tab_id, attempt)
}

#[test]
fn public_send_between_launch_and_readiness_becomes_one_turn_after_bootstrap() {
    let (fixture, tab_id, attempt) = launch_fixture();
    let readiness = Arc::new(ReadinessRegistry::default());
    readiness.arm(attempt, true);
    let events = Arc::new(Mutex::new(Vec::new()));
    let (started_tx, started_rx) = mpsc::channel();
    let wait_readiness = readiness.clone();
    let wait_mux = fixture.service.mux.clone();
    let wait_tab = tab_id.clone();
    let wait_events = events.clone();
    let send = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        let ready = wait_readiness.await_ready(&wait_mux, &wait_tab, Duration::from_secs(2));
        if ready.is_ok() {
            wait_events.lock().push("brief");
        }
        ready
    });

    started_rx.recv().unwrap();
    assert!(events.lock().is_empty());
    events.lock().push("bootstrap");
    let turn = [ChatMsg {
        id: "startup-turn".into(),
        kind: "user".into(),
        content: "startup bootstrap".into(),
        ..Default::default()
    }];
    assert!(readiness.note_turn(
        &fixture.service.mux,
        &tab_id,
        "launch-session",
        "launch-nonce",
        &turn,
    ));

    assert!(send.join().unwrap().is_ok());
    assert_eq!(events.lock().as_slice(), &["bootstrap", "brief"]);
}

#[test]
fn public_send_before_readiness_never_reports_success_without_a_turn() {
    let (fixture, tab_id, attempt) = launch_fixture();
    let readiness = ReadinessRegistry::default();
    readiness.arm(attempt, true);
    let mut payload_writes = 0;

    let result = readiness.await_ready(&fixture.service.mux, &tab_id, Duration::ZERO);
    if result.is_ok() {
        payload_writes += 1;
    }

    assert!(result.is_err());
    assert_eq!(payload_writes, 0);
}
