#[path = "support/codeterm_bin.rs"]
mod codeterm_bin;

use codeterm_types::trace::{Event, Sink};
use serde_json::Value;
use std::process::Command;

fn run(root: &std::path::Path, args: &[&str]) -> std::process::Output {
    let mut command = Command::new(codeterm_bin::codeterm_bin());
    for (key, _) in std::env::vars() {
        if key.starts_with("CODETERM_") {
            command.env_remove(key);
        }
    }
    command
        .env("CODETERM_DATA_DIR", root)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn trace_cli_reads_ordered_flow_without_a_daemon_and_flags_a_gap() {
    let root = tempfile::tempdir().unwrap();
    let mut sink = Sink::open(&root.path().join("logs"), true).unwrap();
    for (index, stage) in [
        "relay.expected",
        "relay.registered",
        "relay.observed",
        "report.emitted",
        "report.written",
    ]
    .iter()
    .enumerate()
    {
        let mut event = Event::new(
            "attempt_fixture",
            stage,
            "worker_fixture",
            "fixture_provider",
            "ok",
        );
        event.ts = index as u64;
        sink.append(&event).unwrap();
    }
    drop(sink);
    let result = run(root.path(), &["--json", "trace", "show", "attempt_fixture"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let chain: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(chain["events"].as_array().unwrap().len(), 5);
    assert_eq!(chain["events"][0]["stage"], "relay.expected");
    assert_eq!(chain["events"][4]["stage"], "report.written");
    assert!(chain["events"]
        .as_array()
        .unwrap()
        .iter()
        .all(|event| event["trace_id"] == "attempt_fixture"));
    assert!(chain["gaps"].as_array().unwrap().is_empty());
    let recent = run(root.path(), &["--json", "trace", "recent", "--limit", "1"]);
    assert!(recent.status.success());
    let recent: Value = serde_json::from_slice(&recent.stdout).unwrap();
    assert_eq!(recent["flows"].as_array().unwrap().len(), 1);
    std::fs::write(
        root.path().join("logs/trace.jsonl"),
        serde_json::to_string(&Event::new(
            "missing",
            "relay.expected",
            "worker_fixture",
            "fixture_provider",
            "pending",
        ))
        .unwrap()
            + "\n",
    )
    .unwrap();
    let missing = run(root.path(), &["trace", "show", "worker_fixture"]);
    assert!(missing.status.success());
    assert!(String::from_utf8(missing.stdout).unwrap().contains("GAP:"));
}

#[test]
fn trace_cli_empty_default_does_not_create_a_trace_file() {
    let root = tempfile::tempdir().unwrap();
    let output = run(root.path(), &["--json", "trace", "recent"]);
    assert!(output.status.success());
    assert!(!root.path().join("logs/trace.jsonl").exists());
    let invalid = run(
        root.path(),
        &["--json", "trace", "show", "id", "--since", "invalid"],
    );
    assert!(!invalid.status.success());
    let error: Value = serde_json::from_slice(&invalid.stdout).unwrap();
    assert_eq!(error["ok"], false);
}

#[test]
fn trace_cli_joins_a_late_source_link_and_flags_a_missing_assistant() {
    let root = tempfile::tempdir().unwrap();
    let mut sink = Sink::open(&root.path().join("logs"), true).unwrap();
    for event in [
        Event::new("send", "send.started", "tab", "provider", "accepted"),
        Event::new("send", "send.written", "tab", "provider", "pty_queue"),
        Event::new(
            "source",
            "transcript.assistant",
            "tab",
            "provider",
            "source_user_turn",
        ),
        Event::new(
            "send",
            "trace.link",
            "tab",
            "provider",
            "exact_append_window",
        )
        .related(Some("source")),
        Event::new("send", "send.delivered", "tab", "provider", "exact_turn"),
        Event::new(
            "missing",
            "send.delivered",
            "other-tab",
            "provider",
            "exact_turn",
        ),
    ] {
        sink.append(&event).unwrap();
    }
    drop(sink);
    let output = run(root.path(), &["--json", "trace", "show", "tab"]);
    assert!(output.status.success());
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(result["events"]
        .as_array()
        .unwrap()
        .iter()
        .all(|event| event["trace_id"] == "send"));
    assert!(result["gaps"].as_array().unwrap().is_empty());
    let output = run(root.path(), &["trace", "show", "other-tab"]);
    assert!(output.status.success());
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("no assistant transcript append"));
}
