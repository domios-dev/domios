#[path = "support/codeterm_bin.rs"]
mod codeterm_bin;

use interprocess::local_socket::{prelude::*, GenericNamespaced, ListenerOptions};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

// macOS clocks tick in microseconds, so parallel tests could mint the same time-based name.
static NEXT_SOCKET: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn run_rest_api(verb: &str, json_mode: bool, result: Value) -> std::process::Output {
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
    let method = format!("rest_api.{verb}");
    let server = std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = BufReader::new(stream.unwrap());
            let mut line = String::new();
            if stream.read_line(&mut line).unwrap() == 0 {
                continue;
            }
            let request: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["method"], method);
            let response = json!({ "jsonrpc": "2.0", "id": request["id"], "result": result });
            writeln!(stream.get_mut(), "{response}").unwrap();
            stream.get_mut().flush().unwrap();
            return;
        }
    });
    let scratch = tempfile::tempdir().unwrap();
    let mut command = Command::new(codeterm_bin::codeterm_bin());
    for (key, _) in std::env::vars() {
        if key.starts_with("CODETERM_") {
            command.env_remove(key);
        }
    }
    command
        .env("CODETERM_SOCKET_PATH", socket)
        .env("CODETERM_DATA_DIR", scratch.path())
        .current_dir(scratch.path())
        .args(["rest-api", verb])
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
        Ok(output) => output.unwrap(),
        Err(error) => {
            if cfg!(windows) {
                let _ = Command::new("taskkill")
                    .args(["/F", "/PID", &pid.to_string()])
                    .status();
            } else {
                let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
            }
            panic!("CLI completion exceeded failure deadline: {error}");
        }
    };
    server.join().unwrap();
    output
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
            assert!(output.stderr.is_empty());
        }
    }
}

#[test]
fn rest_api_restart_failed_status_exits_nonzero_and_status_remains_readable() {
    let failed = json!({ "phase": "failed", "message": "fixture bind failure" });
    let output = run_rest_api("restart", true, failed.clone());
    assert_eq!(output.status.code(), Some(1));
    let output = run_rest_api("status", true, failed.clone());
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        failed
    );
    let output = run_rest_api("restart", true, json!({ "phase": "listening" }));
    assert!(output.status.success());
}
