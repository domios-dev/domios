//! With `--json` every command prints exactly one JSON value on stdout, and every
//! failure is one error envelope with a closed `error.code`, whatever the cause.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Output, Stdio};
use std::sync::{mpsc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

const RUN_DEADLINE: Duration = Duration::from_secs(120);
const WORKERS: usize = 8;

/// A code is a stable machine string: the closed shared set, or a command-specific
/// snake_case code such as `tab_adopt_local_only` or a mesh delivery code.
fn is_stable_code(code: &str) -> bool {
    !code.is_empty()
        && code
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
}

/// Commands whose bare form changes the host or needs the network, not their output surface.
const SWEPT_OUT: [&[&str]; 7] = [
    &["update"],
    &["system", "enable-autostart"],
    &["system", "disable-autostart"],
    &["mem", "dream"],
    &["plugin", "channel", "ensure-default"],
    &["plugin", "channel", "refresh"],
    &["plugin", "update-system"],
];

struct CliEnv {
    data_dir: tempfile::TempDir,
    instance: String,
    stub_port: Option<u16>,
}

impl CliEnv {
    fn new(stub_port: Option<u16>) -> Self {
        Self {
            data_dir: tempfile::tempdir().expect("temp data dir"),
            instance: format!("jsoncontract-{}", std::process::id()),
            stub_port: Some(stub_port.unwrap_or_else(refused_port)),
        }
    }
}

/// Without an explicit port the CLI defaults to the production daemon's port.
fn refused_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind probe port");
    listener.local_addr().expect("probe address").port()
}

fn kill_process(pid: u32) {
    let (program, args) = if cfg!(windows) {
        (
            "taskkill",
            vec!["/F".to_string(), "/PID".into(), pid.to_string()],
        )
    } else {
        ("kill", vec!["-9".to_string(), pid.to_string()])
    };
    let _ = Command::new(program).args(args).output();
}

/// Runs the CLI against a scratch data dir and a deterministic, never-live socket
/// name, so no command can reach a real daemon or a real user directory.
fn run_cli(env: &CliEnv, args: &[String]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_codeterm"));
    for (key, _) in std::env::vars() {
        if key.starts_with("CODETERM_") {
            command.env_remove(key);
        }
    }
    command
        .env("CODETERM_INSTANCE", &env.instance)
        .env("CODETERM_DATA_DIR", env.data_dir.path())
        .env("HOME", env.data_dir.path())
        .env("USERPROFILE", env.data_dir.path())
        .current_dir(env.data_dir.path())
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(port) = env.stub_port {
        command
            .env("CODETERM_API_PORT", port.to_string())
            .env("CODETERM_API_TOKEN", "stub-token");
    }
    let child = command.spawn().expect("spawn codeterm");
    let pid = child.id();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(child.wait_with_output());
    });
    match receiver.recv_timeout(RUN_DEADLINE) {
        Ok(output) => output.expect("collect codeterm output"),
        Err(_) => {
            kill_process(pid);
            panic!("`codeterm {}` exceeded {RUN_DEADLINE:?}", args.join(" "));
        }
    }
}

fn json_args(rest: &[&str]) -> Vec<String> {
    std::iter::once("--json")
        .chain(rest.iter().copied())
        .map(str::to_string)
        .collect()
}

fn stdout_value(output: &Output) -> Result<Value, String> {
    let text = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str::<Value>(text.trim())
        .map_err(|error| format!("stdout is not exactly one JSON value ({error}): {text:?}"))
}

fn check_error_envelope(output: &Output, value: &Value) -> Result<(), String> {
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.trim().is_empty() {
        return Err(format!("--json failure left text on stderr: {stderr:?}"));
    }
    if value.get("ok") != Some(&json!(false)) {
        return Err(format!("failure without ok:false: {value}"));
    }
    let error = value
        .get("error")
        .and_then(Value::as_object)
        .ok_or_else(|| format!("error is not an object: {value}"))?;
    let code = error
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !is_stable_code(code) {
        return Err(format!(
            "error.code {code:?} is not a stable machine string"
        ));
    }
    for field in ["message", "hint"] {
        let text = error.get(field).and_then(Value::as_str).unwrap_or_default();
        if text.trim().is_empty() {
            return Err(format!("error.{field} is empty: {value}"));
        }
    }
    Ok(())
}

fn check_contract(output: &Output) -> Result<Value, String> {
    let value = stdout_value(output)?;
    if output.status.success() {
        if value.get("ok") == Some(&json!(false)) {
            return Err(format!("exit 0 with ok:false: {value}"));
        }
    } else {
        check_error_envelope(output, &value)?;
    }
    Ok(value)
}

struct Node {
    path: Vec<String>,
}

fn collect_nodes(node: &Value, path: &mut Vec<String>, out: &mut Vec<Node>) {
    let skipped = SWEPT_OUT.iter().any(|root| {
        path.len() >= root.len() && path.iter().zip(root.iter()).all(|(a, b)| a.as_str() == *b)
    });
    if skipped {
        return;
    }
    out.push(Node { path: path.clone() });
    for child in node
        .get("subcommands")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        path.push(child["name"].as_str().expect("subcommand name").to_string());
        collect_nodes(child, path, out);
        path.pop();
    }
}

#[test]
fn json_error_contract_all_subcommands() {
    let env = CliEnv::new(None);
    let manifest_run = run_cli(&env, &["cli-manifest".to_string()]);
    assert!(manifest_run.status.success(), "cli-manifest must succeed");
    let manifest: Value =
        serde_json::from_slice(&manifest_run.stdout).expect("cli-manifest prints JSON");
    let mut nodes = Vec::new();
    collect_nodes(&manifest, &mut Vec::new(), &mut nodes);
    assert!(
        nodes.len() > 200,
        "manifest sweep found {} nodes",
        nodes.len()
    );

    let queue = Mutex::new(nodes.iter());
    let violations = Mutex::new(Vec::new());
    let usage_failures = Mutex::new(0usize);
    std::thread::scope(|scope| {
        for _ in 0..WORKERS {
            scope.spawn(|| loop {
                let next = queue.lock().expect("queue").next();
                let Some(node) = next else { break };
                let rest: Vec<&str> = node.path.iter().map(String::as_str).collect();
                let output = run_cli(&env, &json_args(&rest));
                match check_contract(&output) {
                    Err(problem) => violations
                        .lock()
                        .expect("violations")
                        .push(format!("codeterm --json {}: {problem}", rest.join(" "))),
                    Ok(value) => {
                        if output.status.code() == Some(2) {
                            *usage_failures.lock().expect("usage count") += 1;
                            let code = value["error"]["code"].as_str().unwrap_or_default();
                            if code != "invalid_argument" {
                                violations.lock().expect("violations").push(format!(
                                    "codeterm --json {}: exit 2 carried code {code:?}",
                                    rest.join(" ")
                                ));
                            }
                        }
                    }
                }
            });
        }
    });

    let violations = violations.into_inner().expect("violations");
    assert!(
        violations.is_empty(),
        "{} of {} commands break the --json contract:\n{}",
        violations.len(),
        nodes.len(),
        violations.join("\n")
    );
    assert!(
        *usage_failures.lock().expect("usage count") > 50,
        "argument errors must reach the JSON envelope for the commands that require arguments"
    );
}

fn stub_reply(request: &Value) -> Value {
    let method = request["method"].as_str().unwrap_or_default();
    match method {
        "system.identify" => json!({ "result": { "ok": true } }),
        "api.request" => {
            let params = &request["params"];
            let verb = params["method"].as_str().unwrap_or_default();
            let path = params["path"].as_str().unwrap_or_default();
            let (status, body) = if path.contains("dmsg-ok") && verb == "DELETE" {
                (200, r#"{"id":"dmsg-ok","state":"queued"}"#.to_string())
            } else if path.contains("live1") && path.ends_with("formatted=false") {
                (200, r#"{"tab_id":"live1","text":"hello"}"#.to_string())
            } else if path == "/api/mesh/approve" {
                (409, r#"{"error":"request is not pending"}"#.to_string())
            } else if path == "/api/mesh/reject" {
                (404, r#"{"error":"pairing request unknown"}"#.to_string())
            } else {
                (404, "Tab not found or has no PTY".to_string())
            };
            json!({ "result": { "status": status, "headers": {}, "body": body } })
        }
        "tab.inspect" => json!({
            "error": { "code": -32603, "message": "one or more requested Tabs were not found" }
        }),
        other => json!({
            "error": { "code": -32602, "message": format!("{other} target not found: nope") }
        }),
    }
}

fn serve(stream: TcpStream) {
    let Ok(clone) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(clone);
    let mut length = 0usize;
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let header = line.trim_end().to_ascii_lowercase();
        if header.is_empty() {
            break;
        }
        if let Some(value) = header.strip_prefix("content-length:") {
            length = value.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; length];
    if reader.read_exact(&mut body).is_err() {
        return;
    }
    let request: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let payload = stub_reply(&request).to_string();
    let mut stream = stream;
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        payload.len(),
        payload
    );
}

fn start_stub() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind stub daemon");
    let port = listener.local_addr().expect("stub address").port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || serve(stream));
        }
    });
    port
}

struct Row {
    args: &'static [&'static str],
    code: &'static str,
}

#[test]
fn not_found_maps_to_code_end_to_end() {
    let env = CliEnv::new(Some(start_stub()));
    let rows = [
        Row {
            args: &["tab", "get-text", "--tab", "deadbeef"],
            code: "not_found",
        },
        Row {
            args: &["tab", "status", "--tab", "deadbeef"],
            code: "not_found",
        },
        Row {
            args: &["tab", "inspect", "--tab", "deadbeef"],
            code: "not_found",
        },
        Row {
            args: &["send", "--tab", "deadbeef", "x"],
            code: "not_found",
        },
        Row {
            args: &["cron", "history", "nope"],
            code: "not_found",
        },
        Row {
            args: &["mem", "get", "nope"],
            code: "not_found",
        },
        Row {
            args: &["mesh", "reject", "nope"],
            code: "not_found",
        },
        Row {
            args: &["mesh", "approve", "nope"],
            code: "conflict",
        },
        Row {
            args: &["delayed", "delete", "dmsg-nope"],
            code: "not_found",
        },
        Row {
            args: &["plugin", "update", "nosuch"],
            code: "not_found",
        },
        Row {
            args: &["docs", "NOPE"],
            code: "not_found",
        },
    ];
    let mut violations = Vec::new();
    for row in &rows {
        let output = run_cli(&env, &json_args(row.args));
        let label = row.args.join(" ");
        if output.status.success() {
            violations.push(format!("{label}: exited 0 for a missing target"));
            continue;
        }
        match check_contract(&output) {
            Err(problem) => violations.push(format!("{label}: {problem}")),
            Ok(value) => {
                let code = value["error"]["code"].as_str().unwrap_or_default();
                if code != row.code {
                    violations.push(format!("{label}: code {code:?}, wanted {:?}", row.code));
                }
            }
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn json_success_paths_print_one_object() {
    let env = CliEnv::new(Some(start_stub()));

    let deleted = run_cli(&env, &json_args(&["delayed", "delete", "dmsg-ok"]));
    let value = check_contract(&deleted).expect("delayed delete prints one JSON value");
    assert!(deleted.status.success());
    assert_eq!(value["ok"], json!(true));
    assert_eq!(value["deleted"], json!(true));
    assert_eq!(value["id"], "dmsg-ok");
    assert!(
        value.get("state").is_none(),
        "the deleted record is nested, not echoed as a live queued message: {value}"
    );

    let text = run_cli(&env, &json_args(&["tab", "get-text", "--tab", "live1"]));
    let value = check_contract(&text).expect("tab get-text prints one JSON value");
    assert!(text.status.success());
    assert_eq!(value["ok"], json!(true));
    assert_eq!(value["text"], "hello");
    assert_eq!(value["tab_id"], "live1");

    let docs = run_cli(&env, &json_args(&["docs", "API"]));
    let value = check_contract(&docs).expect("docs prints one JSON value");
    assert!(docs.status.success());
    assert!(value["content"]
        .as_str()
        .is_some_and(|body| !body.is_empty()));
}
