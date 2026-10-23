use super::*;
use serde_json::json;

#[test]
fn restart_route_forwards_command_once_and_returns_submission_without_completion() {
    for status in ["submitted", "submit_unconfirmed"] {
        let receipt = json!({
            "tab_id": "tab", "old_pty_id": "old", "new_pty_id": "new",
            "command_delivered": status == "submitted",
            "command_submission": {
                "status": status, "pty_id": "new", "bytes_written": true,
                "bytes_queued": 71, "evidence": "pty_write_queue", "detail": null
            }
        });
        let mut calls = 0;
        let response = restart_shell_request(
            serde_json::from_value(json!({"cmd": "opaque command", "cwd": "/repo"})).unwrap(),
            None,
            |_, _| panic!("no timeout requested"),
            |cwd, command| {
                calls += 1;
                assert_eq!(cwd.as_deref(), Some("/repo"));
                assert_eq!(command, Some("opaque command"));
                Ok(receipt.clone())
            },
        )
        .unwrap()
        .0;
        assert_eq!(calls, 1);
        assert_eq!(response, receipt);
        assert!(response.get("exit_code").is_none());
    }
}

#[test]
fn restart_route_proven_non_submission_is_a_typed_refusal() {
    let result = restart_shell_request::<serde_json::Value>(
        serde_json::from_value(json!({"cmd": "opaque"})).unwrap(),
        None,
        |_, _| panic!("no timeout requested"),
        |_, _| Err("PTY unavailable".into()),
    );
    assert_eq!(
        result.unwrap_err(),
        (
            StatusCode::CONFLICT,
            "shell_command_not_submitted: PTY unavailable".into()
        )
    );
}

#[test]
fn restart_route_refuses_missing_timeout_owner_before_restart() {
    let result = restart_shell_request::<serde_json::Value>(
        serde_json::from_value(json!({"cmd": "opaque", "timeout": "30m"})).unwrap(),
        None,
        |_, _| panic!("no owner"),
        |_, _| panic!("must refuse before restarting"),
    );
    assert_eq!(
        result.unwrap_err(),
        (
            StatusCode::CONFLICT,
            "shell_command_not_submitted: timeout requires an owning agent".into()
        )
    );
}

#[test]
fn restart_route_renews_requested_timeout_before_submission() {
    let calls = std::cell::RefCell::new(Vec::new());
    let _ = restart_shell_request(
        serde_json::from_value(json!({"cmd": "opaque", "timeout": "30m"})).unwrap(),
        Some("owner"),
        |owner, timeout| {
            assert_eq!((owner, timeout), ("owner", "30m"));
            calls.borrow_mut().push("timeout");
            Ok(())
        },
        |_, _| {
            calls.borrow_mut().push("submission");
            Ok(json!({"command_delivered": true}))
        },
    )
    .unwrap();
    assert_eq!(*calls.borrow(), ["timeout", "submission"]);
}
