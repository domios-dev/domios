#[path = "support/codeterm_bin.rs"]
mod codeterm_bin;
#[path = "support/json_output.rs"]
mod json_output;

use interprocess::local_socket::{prelude::*, GenericNamespaced, ListenerOptions};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

// macOS clocks tick in microseconds, so parallel tests could mint the same time-based name.
static NEXT_SOCKET: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn run_rest_api(verb: &str, json_mode: bool, result: Value) -> std::process::Output {
    run_rpc(
        &["rest-api", verb],
        &format!("rest_api.{verb}"),
        json_mode,
        result,
    )
}

fn run_rpc(args: &[&str], method: &str, json_mode: bool, result: Value) -> std::process::Output {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let sequence = NEXT_SOCKET.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let socket = format!(
        "codeterm-rest-api-test-{}-{nonce}-{sequence}.sock",
        std::process::id()
    );
    let listener = ListenerOptions::new()
        .name(socket.clone().to_ns_name::<GenericNamespaced>().unwrap())
        .create_sync()
        .unwrap();
    let method = method.to_owned();
    let stopping = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let server_stopping = stopping.clone();
    let server = std::thread::spawn(move || {
        let mut connections = Vec::new();
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        for stream in listener.incoming() {
            let stream = stream.unwrap();
            if server_stopping.load(std::sync::atomic::Ordering::Acquire) {
                break;
            }
            let method = method.clone();
            let result = result.clone();
            let calls = calls.clone();
            connections.push(std::thread::spawn(move || {
                let mut stream = BufReader::new(stream);
                loop {
                    let mut line = String::new();
                    if stream.read_line(&mut line).unwrap() == 0 {
                        return;
                    }
                    let request: Value = serde_json::from_str(&line).unwrap();
                    let admission = request["method"] == "cli.local.authorize";
                    let result = if admission {
                        json!({ "authorized": true })
                    } else {
                        assert_eq!(request["method"], method);
                        assert_eq!(calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst), 0);
                        result.clone()
                    };
                    let response =
                        json!({ "jsonrpc": "2.0", "id": request["id"], "result": result });
                    writeln!(stream.get_mut(), "{response}").unwrap();
                    stream.get_mut().flush().unwrap();
                    if !admission {
                        return;
                    }
                }
            }));
        }
        for connection in connections {
            connection.join().unwrap();
        }
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    });
    let scratch = tempfile::tempdir().unwrap();
    let mut command = codeterm_bin::command();
    command
        .env("CODETERM_SOCKET_PATH", &socket)
        .env("CODETERM_API_TOKEN", "fixture-host-token")
        .env("CODETERM_DATA_DIR", scratch.path())
        .current_dir(scratch.path())
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if json_mode {
        command.arg("--json");
    }
    let child = command.spawn().unwrap();
    let pid = child.id();
    let (completed, completion) = mpsc::channel();
    std::thread::spawn(move || completed.send(child.wait_with_output()).unwrap());
    let output = match completion.recv_timeout(Duration::from_secs(15)) {
        Ok(output) => output,
        Err(error) => {
            if cfg!(windows) {
                let _ = Command::new("taskkill")
                    .args(["/F", "/PID", &pid.to_string()])
                    .status();
            } else {
                let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
            }
            Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("CLI completion exceeded failure deadline: {error}"),
            ))
        }
    };
    stopping.store(true, std::sync::atomic::Ordering::Release);
    let _wake = interprocess::local_socket::Stream::connect(
        socket.to_ns_name::<GenericNamespaced>().unwrap(),
    )
    .unwrap();
    server.join().unwrap();
    output.unwrap()
}

#[test]
fn rest_api_restart_failure_envelope_exits_nonzero_in_both_output_modes() {
    for json_mode in [false, true] {
        let output = run_rest_api(
            "restart",
            json_mode,
            json!({ "ok": false, "error": { "message": "fixture health failure", "rpc_code": -32603 } }),
        );
        assert_eq!(output.status.code(), Some(1));
        if json_mode {
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value["ok"], false);
            assert_eq!(value["error"]["rpc_code"], -32603);
        }
        assert!(String::from_utf8_lossy(&output.stderr).contains("fixture health failure"));
    }
}

#[test]
fn daemon_failure_exits_nonzero_before_command_specific_rendering() {
    let failure = json!({
        "ok": false,
        "error": { "code": "internal", "message": "fixture daemon failure" },
        "result": { "receipt": "retained" }
    });
    for (args, method) in [
        (
            vec!["agent", "spawn", "fixture", "--description", "fixture"],
            "agent.spawn",
        ),
        (
            vec!["workspace", "close", "fixture", "--force"],
            "workspace.close",
        ),
        (vec!["tab", "list"], "api.request"),
    ] {
        for json_mode in [false, true] {
            let output = run_rpc(&args, method, json_mode, failure.clone());
            assert_eq!(output.status.code(), Some(1), "{args:?}: {output:?}");
            assert!(String::from_utf8_lossy(&output.stderr).contains("fixture daemon failure"));
            if json_mode {
                assert_eq!(
                    serde_json::from_slice::<Value>(&output.stdout).unwrap(),
                    failure
                );
            } else {
                assert!(output.stdout.is_empty(), "{args:?}: {output:?}");
            }
        }
    }
}

#[test]
fn daemon_success_keeps_nested_failures_as_payload() {
    for result in [
        json!({ "ok": true, "data": { "ok": false } }),
        json!({ "error": "historical failure", "phase": "listening" }),
    ] {
        let output = run_rest_api("status", true, result.clone());
        assert!(output.status.success(), "{output:?}");
        assert_eq!(json_output::result(&output.stdout), result);
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn daemon_api_failure_exits_nonzero_through_shared_response_handling() {
    let failure = json!({ "ok": false, "error": "fixture API failure" });
    for json_mode in [false, true] {
        let output = run_rpc(
            &["tab", "status", "--tab", "fixture"],
            "api.request",
            json_mode,
            json!({ "status": 200, "headers": {}, "body": failure.to_string() }),
        );
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("fixture API failure"));
        if json_mode {
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value["ok"], false);
            assert_eq!(value["error"], failure["error"]);
        } else {
            assert!(output.stdout.is_empty());
        }
    }
}

#[test]
fn plan_reindex_renders_counts_and_scope_while_plan_list_renders_rows() {
    let reindex = json!({
        "project": "/fixture/project",
        "scope": "project:fixture",
        "plans": 1,
        "specs": 1,
        "units": 26
    });
    let output = run_rpc(
        &["plan", "reindex", "--project", "/fixture/project"],
        "plan.reindex",
        false,
        reindex,
    );
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    let fields: Vec<Vec<&str>> = text
        .lines()
        .map(|line| line.split_whitespace().collect())
        .collect();
    for expected in [
        vec!["project", "/fixture/project"],
        vec!["scope", "project:fixture"],
        vec!["plans", "1"],
        vec!["specs", "1"],
        vec!["units", "26"],
    ] {
        assert!(fields.contains(&expected), "{text}");
    }
    assert!(!text.contains("(no plans)"), "{text}");

    let output = run_rpc(
        &["plan", "list"],
        "plan.list",
        false,
        json!({ "plans": [{ "plan_id": "pl_fixture", "title": "Fixture plan" }] }),
    );
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("pl_fixture"), "{text}");
    assert!(text.contains("Fixture plan"), "{text}");
    assert!(!text.contains("(no plans)"), "{text}");
}

#[test]
fn rest_api_restart_failed_status_exits_nonzero_and_status_remains_readable() {
    let failed = json!({ "phase": "failed", "message": "fixture bind failure" });
    let output = run_rest_api("restart", true, failed.clone());
    assert_eq!(output.status.code(), Some(1));
    let output = run_rest_api("status", true, failed.clone());
    assert!(output.status.success());
    assert_eq!(json_output::result(&output.stdout), failed);
    let output = run_rest_api("restart", true, json!({ "phase": "listening" }));
    assert!(output.status.success());
}
