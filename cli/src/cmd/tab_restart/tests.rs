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
        Err("delivery timeout".into())
    });
    assert_eq!(result.unwrap_err(), "delivery timeout");
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
fn restart_requires_an_explicit_command_delivery_receipt() {
    for receipt in [json!({}), json!({"command_delivered": false})] {
        let result =
            restart_with_command("http://local", "tab", &json!({}), Some("opaque"), |_, _| {
                Ok(receipt.clone())
            });
        assert!(result.unwrap_err().contains("not acknowledged"));
    }
}
