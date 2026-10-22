use super::*;
use crate::cmd::print::CliError;

#[test]
fn commands_wait_before_submission_but_recovery_refuses_starting() {
    let starting = json!({"state":"starting", "phases":{"service":"starting"}});
    assert!(!may_proceed(&starting, true).unwrap());
    let refusal = CliError::new(may_proceed(&starting, false).unwrap_err());
    assert_eq!(refusal.message, "Domios is starting (phase service)");
    assert!(may_proceed(&json!({"state":"ready"}), true).unwrap());
    assert!(may_proceed(&json!({"state":"ready"}), false).unwrap());
    assert!(may_proceed(&json!({"state":"failed"}), true).is_err());
}

#[test]
fn starting_refusal_keeps_phase_and_machine_code_without_credentials() {
    let snapshot = json!({"state":"starting", "phases":{
        "plans":"starting", "service":"ready", "providers":"ready",
        "states":"ready", "integrity":"ready", "listener":"ready",
        "background":"starting", "platform":"starting",
    }});
    let error = CliError::new(snapshot_error(&snapshot));
    assert_eq!(error.message, "Domios is starting (phase plans)");
    let envelope =
        crate::cmd::error_contract::error_envelope(&error.message, error.code, error.data.as_ref());
    assert_eq!(envelope["error"]["code"], "starting");
    assert_eq!(envelope["data"]["startup"], snapshot);
}

#[test]
fn failed_startup_remains_a_terminal_typed_refusal() {
    let error = CliError::new(snapshot_error(
        &json!({"state":"failed", "phases":{"integrity":"failed"}}),
    ));
    let envelope =
        crate::cmd::error_contract::error_envelope(&error.message, error.code, error.data.as_ref());
    assert_eq!(envelope["error"]["code"], "startup_failed");
    assert_eq!(envelope["data"]["startup"]["phases"]["integrity"], "failed");
}

#[test]
fn listener_eof_preserves_latest_phase_until_next_stream_is_ready() {
    let mut snapshot = json!({"state":"starting", "phases":{"listener":"starting"}});
    let first = b"event: startup-state\r\ndata: {\"state\":\"starting\",\"phases\":{\"plans\":\"starting\"}}\r\n\r\n";
    assert!(!read_events(&first[..], &mut snapshot).unwrap());
    assert_eq!(current_phase(&snapshot), Some("plans"));
    let second = b": keepalive\n\nevent: other\ndata: ignored\n\nevent: startup-state\ndata: {\"state\":\"ready\"}\n\n";
    assert!(read_events(&second[..], &mut snapshot).unwrap());
    assert_eq!(snapshot["state"], "ready");
    assert!(refusal(&json!({"startup":snapshot})).is_none());
}

#[test]
fn startup_stream_failure_does_not_become_ready() {
    let mut snapshot = json!({"state":"starting"});
    let failed = b"event: startup-state\ndata: {\"state\":\"failed\"}\n\n";
    assert!(read_events(&failed[..], &mut snapshot).unwrap());
    assert_eq!(snapshot["state"], "failed");
    assert!(read_events(
        &b"event: startup-state\ndata: invalid\n\n"[..],
        &mut snapshot
    )
    .is_err());
}
