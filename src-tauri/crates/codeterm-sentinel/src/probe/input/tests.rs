use super::*;
use crate::probe::{
    environment::Environment, owner::ProbeResourceOwner, run_with_budget_input, PROBE_RUNTIME,
};
use serde_json::{json, Value};
use std::sync::Arc;

fn run(command: &str, previous: &str) -> crate::probe::Output {
    let dir = tempfile::tempdir().unwrap();
    run_with_budget_input(
        command,
        dir.path(),
        PROBE_RUNTIME,
        PROBE_RUNTIME,
        &Arc::new(ProbeResourceOwner::new()),
        &Environment::default(),
        previous,
    )
    .unwrap()
}

#[cfg(windows)]
const READ: &str = "[Console]::Out.Write([Console]::In.ReadToEnd())";
#[cfg(unix)]
const READ: &str = "cat";

#[test]
fn blueprint_command_stdin_preserves_typed_result_and_eof() {
    let previous =
        json!({"source_state":"failing", "failed_jobs":["opaque $(exit 17) `exit 19`"], "count":3});
    let output = run(READ, &previous.to_string());
    assert!(output.success);
    assert_eq!(
        serde_json::from_str::<Value>(&output.stdout_text()).unwrap(),
        json!({"media":"json","schema_id":null,"value":previous,"text":""})
    );
    let output = run(READ, "opaque plain text");
    assert_eq!(
        ExecutionEnvelope::decode(output.stdout_text().as_bytes())
            .unwrap()
            .text,
        "opaque plain text"
    );
}

#[test]
fn blueprint_command_stdin_bounds_oversized_result_with_typed_marker() {
    for text in [
        "x".repeat(100_000),
        "\u{0000}".repeat(20_000),
        "界".repeat(40_000),
    ] {
        assert_eq!(
            encode(&text).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}

#[test]
fn blueprint_command_ignoring_stdin_completes() {
    let output = run("echo completed", "opaque");
    assert!(output.success);
    assert_eq!(output.stdout_text().trim(), "completed");
}

#[test]
fn blueprint_v2_runtime_stdin_exact_boundary_and_argv_separation() {
    let empty = ExecutionEnvelope::from_text("").encode().unwrap().len();
    let payload = "x".repeat((32768 - empty) / 2);
    let encoded = encode(&payload).unwrap();
    assert!(encoded.len() <= 32768);
    assert!(encode(&(payload + "x")).is_err());
    let opaque = "$(touch forbidden) `exit 19` ; exit 21";
    #[cfg(unix)]
    let command = "printf '%s\\n' \"$#\"; cat";
    #[cfg(windows)]
    let command =
        "[Console]::Out.WriteLine($args.Count); [Console]::Out.Write([Console]::In.ReadToEnd())";
    let output = run(command, opaque);
    assert!(output.success);
    let stdout = output.stdout_text();
    let (argc, stdin) = stdout.split_once('\n').unwrap();
    assert_eq!(argc.trim(), "0");
    assert_eq!(
        ExecutionEnvelope::decode(stdin.as_bytes()).unwrap().text,
        opaque
    );
}

#[test]
fn blueprint_v2_runtime_oversized_stdin_refuses_before_command_effect() {
    let dir = tempfile::tempdir().unwrap();
    let result = run_with_budget_input(
        "echo effect",
        dir.path(),
        PROBE_RUNTIME,
        PROBE_RUNTIME,
        &Arc::new(ProbeResourceOwner::new()),
        &Environment::default(),
        &"x".repeat(32768),
    );
    assert_eq!(result.unwrap_err().primary, "contract_violation");
}
