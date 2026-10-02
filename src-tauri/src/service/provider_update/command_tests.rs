use super::*;

#[test]
fn provider_update_output_is_drained_with_a_bounded_tail() {
    let mut bytes = vec![b'x'; 65536];
    bytes.extend(b"opaque-final-output");
    let rx = capture(std::io::Cursor::new(bytes)).unwrap();
    let output = captured(Some(rx));
    assert_eq!(output.len(), 32768);
    assert!(output.ends_with("opaque-final-output"));
}

#[test]
fn provider_update_command_rejects_shell_input() {
    assert!(update_argv("updater update; injected").is_err());
    assert!(update_argv("updater $(injected)").is_err());
    assert!(update_argv("").is_err());
}

#[test]
fn provider_update_command_real_success_waits_for_normal_children() {
    let mut fixture = process_fixture::Fixture::start("success");
    fixture.release("descendant");
    fixture.release("parent");
    assert!(fixture.finish().is_ok());
}

#[test]
fn provider_update_command_parent_zero_with_closed_stream_live_child_is_unconfirmed() {
    let mut fixture = process_fixture::Fixture::start("closed");
    fixture.release("parent");
    let failure = fixture.result.recv_timeout(std::time::Duration::from_secs(10)).unwrap().err().expect("parent exit zero is insufficient");
    assert!(!failure.child_stopped);
}

#[test]
fn provider_update_command_parent_zero_with_inherited_stream_live_child_is_unconfirmed() {
    let mut fixture = process_fixture::Fixture::start("inherited");
    fixture.release("parent");
    let failure = fixture.result.recv_timeout(std::time::Duration::from_secs(10)).unwrap().err().expect("remaining child was killed to manufacture success");
    assert!(!failure.child_stopped);
}

#[test]
fn provider_update_command_real_nonzero_retains_tail_and_never_restarts() {
    let mut fixture = process_fixture::Fixture::start("nonzero");
    fixture.release("descendant");
    fixture.release("parent");
    let failure = fixture.finish().err().unwrap();
    assert!(failure.child_stopped);
    assert!(failure.reason.contains("fixture-error-tail"));
    assert!(!failure.reason.contains(&"x".repeat(32769)));
    let mut state = super::super::state::Coordinator::default();
    state.begin("fixture", "1", "2");
    state.finish("fixture:2", Err(failure.reason), &["idle".into()]);
    assert!(!state.claim_restart("fixture:2", "idle", super::super::state::TabReadiness { idle: true, active_turn: false, active_attempt: false }));
    assert!(state.claim_notice("fixture:2"));
    assert!(!state.claim_notice("fixture:2"));
}
