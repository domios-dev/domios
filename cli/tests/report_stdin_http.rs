#[path = "support/codeterm_bin.rs"]
mod codeterm_bin;

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const REPORT: &str = "UTF-8 report: \u{1f9ea}\nsecond line with `$literal`\n";

fn request(stream: &TcpStream) -> Option<(HashMap<String, String>, Value)> {
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    if reader.read_line(&mut line).ok()? == 0 {
        return None;
    }
    assert!(line.starts_with("POST /api/cli/rpc "));
    let mut headers = HashMap::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        if line.trim().is_empty() {
            break;
        }
        let (key, value) = line.trim_end().split_once(':').unwrap();
        headers.insert(key.to_ascii_lowercase(), value.trim().to_string());
    }
    let length = headers.get("content-length")?.parse().ok()?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body).ok()?;
    Some((headers, serde_json::from_slice(&body).unwrap()))
}

fn reply(stream: &mut TcpStream, accepted: bool, payload: Value) {
    let body = payload.to_string();
    let status = if accepted { "200 OK" } else { "403 Forbidden" };
    write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    stream.flush().unwrap();
}

#[test]
fn stdin_taskless_report_keeps_http_fallback_identity_and_utf8_payload() {
    for session in [None, Some("own-session")] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut identify_probes = 0;
            for incoming in listener.incoming() {
                let mut stream = incoming.unwrap();
                let Some((headers, body)) = request(&stream) else {
                    return None;
                };
                assert_eq!(
                    headers.get("authorization").map(String::as_str),
                    Some("Bearer fixture-token")
                );
                if body["method"] == "system.identify" {
                    identify_probes += 1;
                    assert_eq!(identify_probes, 1);
                    reply(&mut stream, true, json!({"result":{"ok":true}}));
                    continue;
                }
                if body["method"] == "plan.binding.resolve" {
                    reply(&mut stream, true, json!({"result":{"binding":null}}));
                    continue;
                }
                assert_eq!(body["method"], "agent.report");
                assert_eq!(identify_probes, 1);
                let params = &body["params"];
                let matches = [
                    ("x-codeterm-tab-id", "tab_id"),
                    ("x-codeterm-tab-id", "caller_tab_id"),
                    ("x-codeterm-session-id", "caller_session_id"),
                    ("x-codeterm-binding-nonce", "session_binding_nonce"),
                ]
                .into_iter()
                .all(|(header, field)| {
                    headers.get(header).map(String::as_str) == params[field].as_str()
                });
                let response = if matches {
                    json!({"result":{"ok":true,"accepted":true,"state":"delivered","target_tab_id":"fixture-parent"}})
                } else {
                    json!({"error":{"code":-32003,"message":"HTTP RPC identity headers do not match request parameters","data":null}})
                };
                reply(&mut stream, matches, response);
                return Some(body);
            }
            None
        });
        let scratch = tempfile::tempdir().unwrap();
        std::fs::write(
            scratch.path().join("config.toml"),
            format!("[api]\nhost = \"127.0.0.1\"\nport = {}\n", address.port()),
        )
        .unwrap();
        let mut command = Command::new(codeterm_bin::codeterm_bin());
        for (key, _) in std::env::vars() {
            if key.starts_with("CODETERM_") {
                command.env_remove(key);
            }
        }
        command
            .env("CODETERM_DATA_DIR", scratch.path())
            .env(
                "CODETERM_SOCKET_PATH",
                format!("codeterm-report-{}.sock", uuid::Uuid::new_v4()),
            )
            .env("CODETERM_API_URL", format!("http://{address}"))
            .env("CODETERM_API_TOKEN", "fixture-token")
            .env("CODETERM_TAB_ID", "fixture-worker")
            .env("CODETERM_SESSION_BINDING_NONCE", "own-nonce")
            .current_dir(scratch.path())
            .args(["--json", "agent", "report", "--from-pad", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(session) = session {
            command.env("CODETERM_SESSION_ID", session);
        }
        let mut child = command.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(REPORT.as_bytes())
            .unwrap();
        let pid = child.id();
        let (completed, completion) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = completed.send(child.wait_with_output());
        });
        let output = completion.recv_timeout(Duration::from_secs(15));
        if output.is_err() {
            if cfg!(windows) {
                let _ = Command::new("taskkill")
                    .args(["/F", "/PID", &pid.to_string()])
                    .status();
            } else {
                let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
            }
        }
        if let Ok(wake) = TcpStream::connect(address) {
            let _ = wake.shutdown(Shutdown::Both);
        }
        let observed = server.join().unwrap();
        let output = output
            .expect("CLI report exceeded failure deadline")
            .unwrap();
        assert!(
            output.status.success(),
            "status: {}\nstdout: {}\nstderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let delivered: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(delivered["state"], "delivered");
        assert_eq!(observed.unwrap()["params"]["message"], REPORT);
    }
}
