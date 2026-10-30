use super::*;
use crate::cmd::print::CliError;

#[test]
fn startup_discovery_preserves_the_selected_instance_address_boundary() {
    let mut selected = crate::cmd::identify::ResolvedApiConfig {
        data_dir: std::path::PathBuf::from("selected-dev"),
        data_dir_source: super::super::ResolutionSource::CodeTermInstance,
        host: "127.0.0.1".into(),
        host_source: super::super::ResolutionSource::Default,
        port: 7685,
        port_source: super::super::ResolutionSource::Default,
        token: None,
    };
    assert!(target_url_from(&selected, Some("http://127.0.0.1:7685"), None, None, true).is_err());
    selected.port = 7791;
    selected.port_source = super::super::ResolutionSource::Config;
    assert!(target_url_from(&selected, Some("http://127.0.0.1:7685"), None, None, true).is_err());
    assert_eq!(
        target_url_from(&selected, Some("http://127.0.0.1:7791"), None, None, true).unwrap(),
        "http://127.0.0.1:7791"
    );
}

#[test]
fn ready_rpc_probe_does_not_subscribe_to_startup() {
    probe_with_readiness(
        || Ok((reqwest::StatusCode::OK, json!({"result":{"ok":true}}))),
        |_| panic!("ready RPC must not incur a readiness round trip"),
    )
    .unwrap();
}

#[test]
fn starting_rpc_waits_then_repeats_only_the_identify_probe() {
    let steps = std::cell::RefCell::new(Vec::new());
    let probes = std::cell::Cell::new(0);
    probe_with_readiness(
        || {
            steps.borrow_mut().push("identify");
            probes.set(probes.get() + 1);
            if probes.get() == 1 {
                Ok((
                    reqwest::StatusCode::SERVICE_UNAVAILABLE,
                    json!({"startup":{"state":"starting","phases":{"plans":"starting"}}}),
                ))
            } else {
                Ok((reqwest::StatusCode::OK, json!({"result":{"ok":true}})))
            }
        },
        |snapshot| {
            assert_eq!(snapshot["phases"]["plans"], "starting");
            steps.borrow_mut().push("startup-events");
            Ok(())
        },
    )
    .unwrap();
    steps.borrow_mut().push("user-command");
    assert_eq!(
        *steps.borrow(),
        ["identify", "startup-events", "identify", "user-command"]
    );
}

#[test]
fn failed_startup_probe_never_waits_or_retries() {
    let error = probe_with_readiness(
        || {
            Ok((
                reqwest::StatusCode::SERVICE_UNAVAILABLE,
                json!({"startup":{"state":"failed"}}),
            ))
        },
        |_| panic!("terminal startup failure cannot be retried"),
    )
    .unwrap_err();
    assert_eq!(
        CliError::new(error).data.unwrap()["error"]["code"],
        "startup_failed"
    );
}

#[test]
fn failed_readiness_wait_prevents_a_second_probe() {
    let probes = std::cell::Cell::new(0);
    let error = probe_with_readiness(
        || {
            probes.set(probes.get() + 1);
            Ok((
                reqwest::StatusCode::SERVICE_UNAVAILABLE,
                json!({"startup":{"state":"starting"}}),
            ))
        },
        |_| Err("readiness deadline".into()),
    )
    .unwrap_err();
    assert_eq!(error, "readiness deadline");
    assert_eq!(probes.get(), 1);
}

#[test]
fn replaced_startup_after_ready_is_returned_without_another_wait() {
    let waits = std::cell::Cell::new(0);
    let probes = std::cell::Cell::new(0);
    let error = probe_with_readiness(
        || {
            probes.set(probes.get() + 1);
            Ok((
                reqwest::StatusCode::SERVICE_UNAVAILABLE,
                json!({"startup":{"state":"starting","phases":{"service":"starting"}}}),
            ))
        },
        |_| {
            waits.set(waits.get() + 1);
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(
        CliError::new(error).data.unwrap()["error"]["code"],
        "starting"
    );
    assert_eq!(waits.get(), 1);
    assert_eq!(probes.get(), 2);
}

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
    assert_eq!(envelope["result"]["startup"], snapshot);
}

#[test]
fn failed_startup_remains_a_terminal_typed_refusal() {
    let error = CliError::new(snapshot_error(
        &json!({"state":"failed", "phases":{"integrity":"failed"}}),
    ));
    let envelope =
        crate::cmd::error_contract::error_envelope(&error.message, error.code, error.data.as_ref());
    assert_eq!(envelope["error"]["code"], "startup_failed");
    assert_eq!(
        envelope["result"]["startup"]["phases"]["integrity"],
        "failed"
    );
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
