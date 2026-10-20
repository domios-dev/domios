use super::*;
use crate::agent::registry::AgentRegistry;
use crate::multiplexer::state::MultiplexerState;
use codeterm_util::lock_hold::TimedRwLock;
use std::sync::{mpsc, Arc};
use std::time::Duration;

fn fixture() -> (MuxState, String) {
    let (mut state, id) = MultiplexerState::new();
    let tab = state.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("fixture".into());
    tab.agent_session_id = Some("own".into());
    tab.session_binding_nonce = Some("nonce".into());
    tab.reported_session_binding_nonce = Some("nonce".into());
    (Arc::new(TimedRwLock::new("mux", state)), id)
}

#[test]
fn pure_model_update_preserves_turn_confirmation_and_plans_notice() {
    let (mux, id) = fixture();
    let mut snapshot = ModelSnapshot::capture(mux.read().get_tab(&id).unwrap());
    snapshot.requested = Some("requested".into());
    let (values, notice) = compute(&snapshot, "own", "actual", None, true, false);
    assert!(notice.model_changed);
    assert!(notice.state_changed);
    assert!(notice.should_notify);
    assert_eq!(values.model.as_deref(), Some("actual"));
    assert!(
        values
            .identity
            .as_ref()
            .unwrap()
            .model_switch_turn_confirmed
    );
    assert_eq!(values.mismatch.as_ref().unwrap().actual_model, "actual");
    snapshot.values = values;
    let (repeated, notice) = compute(&snapshot, "own", "actual", None, false, false);
    assert!(!notice.model_changed);
    assert!(!notice.should_notify);
    assert!(
        repeated
            .identity
            .as_ref()
            .unwrap()
            .model_switch_turn_confirmed
    );
}

#[test]
fn snapshot_refuses_changed_identity_or_binding() {
    let (mux, id) = fixture();
    let snapshot = ModelSnapshot::capture(mux.read().get_tab(&id).unwrap());
    let mut state = mux.write();
    let tab = state.get_tab_mut(&id).unwrap();
    tab.agent_session_model = Some("concurrent".into());
    assert!(!snapshot.matches(tab));
    tab.agent_session_model = None;
    tab.session_binding_nonce = Some("other".into());
    assert!(!snapshot.matches(tab));
}

#[test]
fn identical_model_and_identity_skip_write_even_with_an_existing_reader() {
    let (mux, id) = fixture();
    let registry = AgentRegistry::new();
    update(
        &mux,
        &registry,
        &id,
        "fixture",
        "own",
        "actual",
        None,
        Some("nonce"),
        true,
    )
    .unwrap();
    let reader = mux.read();
    let worker_mux = mux.clone();
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let result = update(
            &worker_mux,
            &registry,
            &id,
            "fixture",
            "own",
            "actual",
            None,
            Some("nonce"),
            false,
        )
        .unwrap();
        tx.send(result.state_changed).unwrap();
    });
    let result = rx.recv_timeout(Duration::from_secs(5));
    drop(reader);
    worker.join().unwrap();
    assert!(!result.expect("no-op must finish without acquiring mux.write"));
}
