use super::*;
use serde_json::json;

#[test]
fn restart_delivers_command_without_waiting_for_completion() {
    let command = r"ping -t 127.0.0.1";
    let mut calls = 0;
    let result = restart_with_command(
        "http://local",
        "tab",
        &json!({}),
        Some(command),
        |url, body| {
            calls += 1;
            assert_eq!(url, "http://local/api/tabs/tab/restart");
            assert_eq!(body["cmd"], command);
            Ok(json!({"command_delivered": true}))
        },
    )
    .unwrap();
    assert_eq!(calls, 1);
    assert_eq!(result["command_delivered"], true);
}

#[test]
fn failed_restart_does_not_execute_command() {
    let result = restart_with_command("http://local", "tab", &json!({}), Some("opaque"), |_, _| {
        Err(RestartRequestError::Refused(
            "shell_command_not_submitted: PTY absent".into(),
        ))
    });
    assert_eq!(
        result.unwrap_err(),
        "shell_command_not_submitted: PTY absent"
    );
}

#[test]
fn corrupted_command_is_refused_before_restarting_shell() {
    assert!(restart_with_command(
        "http://local",
        "tab",
        &json!({}),
        Some("a\u{8}b"),
        |_, _| panic!("invalid command")
    )
    .is_err());
}

#[test]
fn command_json_preserves_literal_backslashes_and_control_characters() {
    for command in [
        r"\b",
        r"\n",
        r"\t",
        r".\build.ps1",
        r"C:\new\test",
        "one\ntwo\tthree",
        r"\\host\build",
    ] {
        restart_with_command(
            "http://local",
            "tab",
            &json!({}),
            Some(command),
            |_, body| {
                let decoded: serde_json::Value =
                    serde_json::from_slice(&serde_json::to_vec(body).unwrap()).unwrap();
                assert_eq!(
                    decoded["cmd"].as_str().unwrap().as_bytes(),
                    command.as_bytes()
                );
                Ok(json!({"command_delivered": true}))
            },
        )
        .unwrap();
    }
}

#[test]
fn restart_without_command_only_replaces_shell() {
    let result = restart_with_command(
        "http://local",
        "tab",
        &json!({"cwd": "/opaque"}),
        None,
        |_, body| {
            assert_eq!(body, &json!({"cwd": "/opaque"}));
            Ok(json!({"command_delivered": false}))
        },
    )
    .unwrap();
    assert_eq!(result["command_delivered"], false);
}

#[test]
fn missing_command_receipt_is_uncertainty_without_a_replay() {
    for receipt in [
        json!({}),
        json!({"command_delivered": false}),
        json!("invalid"),
    ] {
        let mut calls = 0;
        let result =
            restart_with_command("http://local", "tab", &json!({}), Some("opaque"), |_, _| {
                calls += 1;
                Ok(receipt.clone())
            });
        let result = result.unwrap();
        assert_eq!(calls, 1);
        assert_eq!(result["command_submission"]["status"], "submit_unconfirmed");
        assert_eq!(result["command_submission"]["replay_allowed"], false);
    }
}

#[test]
fn daemon_proven_non_submission_is_refused() {
    let result = restart_with_command("http://local", "tab", &json!({}), Some("opaque"), |_, _| {
        Ok(json!({"command_submission": {"status": "refused", "bytes_written": false}}))
    });
    assert!(result
        .unwrap_err()
        .starts_with("shell_command_not_submitted:"));
}

#[test]
fn lost_response_never_reports_a_possibly_started_command_as_failed() {
    let mut calls = 0;
    let result = restart_with_command("http://local", "tab", &json!({}), Some("opaque"), |_, _| {
        calls += 1;
        Err(RestartRequestError::Unconfirmed(
            "response lost after submission".into(),
        ))
    })
    .unwrap();
    assert_eq!(calls, 1);
    assert_eq!(result["command_submission"]["status"], "submit_unconfirmed");
    assert_eq!(
        result["command_submission"]["bytes_written"],
        serde_json::Value::Null
    );
}

#[test]
fn daemon_postwrite_uncertainty_preserves_its_evidence() {
    let receipt = json!({"command_delivered": false, "command_submission": {
        "status": "submit_unconfirmed", "pty_id": "exact-new-pty", "bytes_written": true,
        "bytes_queued": 53, "evidence": "pty_write_queue", "detail": "submit key refused"
    }});
    let result = restart_with_command("http://local", "tab", &json!({}), Some("opaque"), |_, _| {
        Ok(receipt.clone())
    })
    .unwrap();
    assert_eq!(result, receipt);
}
