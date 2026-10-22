use super::*;
use crate::probe::{
    environment::Environment, owner::ProbeResourceOwner, run_with_budget_input, PROBE_RUNTIME,
};
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
        previous
    );
    let output = run(READ, "opaque plain text");
    assert_eq!(
        serde_json::from_str::<Value>(&output.stdout_text()).unwrap(),
        json!("opaque plain text")
    );
}

#[test]
fn blueprint_command_stdin_bounds_oversized_result_with_typed_marker() {
    for text in [
        "x".repeat(100_000),
        "\u{0000}".repeat(20_000),
        "界".repeat(40_000),
    ] {
        let encoded = encode(&text);
        assert!(encoded.len() <= crate::probe::capture::PREFIX_BYTES);
        let value: Value = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(value["type"], "truncated");
        assert_eq!(value["truncated"], true);
        assert_eq!(value["total_bytes"], text.len());
    }
    let output = run(READ, &"x".repeat(100_000));
    assert!(output.success);
    let value: Value = serde_json::from_str(&output.stdout_text()).unwrap();
    assert_eq!(value["type"], "truncated");
}

#[test]
fn blueprint_command_ignoring_stdin_completes() {
    let output = run("echo completed", &"x".repeat(100_000));
    assert!(output.success);
    assert_eq!(output.stdout_text().trim(), "completed");
}
