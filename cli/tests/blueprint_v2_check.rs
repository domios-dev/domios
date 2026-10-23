#[path = "support/codeterm_bin.rs"]
mod codeterm_bin;
use serde_json::Value;

fn cli(path: &std::path::Path, options: &[&str]) -> std::process::Output {
    let output = std::process::Command::new(codeterm_bin::codeterm_bin())
        .args(["blueprint", "check", path.to_str().unwrap()])
        .args(options)
        .env(
            "CODETERM_SOCKET_PATH",
            format!("absent-{}", uuid::Uuid::new_v4()),
        )
        .env("CODETERM_API_URL", "http://127.0.0.1:1")
        .env_remove("CODETERM_API_TOKEN")
        .output()
        .unwrap();
    use std::io::Write;
    writeln!(
        std::io::stdout().lock(),
        "$ domios blueprint check {} {}\nexit={}\n{}{}",
        path.display(),
        options.join(" "),
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .unwrap();
    output
}
#[test]
fn blueprint_v2_check_real_cli_proof_fix_and_idempotence() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("repair.yaml");
    let source = include_str!("../../tests/fixtures/blueprint-v2-check.yaml");
    std::fs::write(&path, source).unwrap();
    let broken = cli(&path, &["--json", "--explain"]);
    assert_eq!(broken.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&broken.stdout).unwrap();
    let errors = value["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["severity"] == "error")
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 1, "{value}");
    assert_eq!(errors[0]["rule"], "S2");
    assert_eq!(errors[0]["field"], "trigger.every");
    assert_eq!(errors[0]["line"], 5);
    assert!(!errors[0]["proof"]["children"]
        .as_array()
        .unwrap()
        .is_empty());
    let explain = cli(&path, &["--explain"]);
    assert_eq!(explain.status.code(), Some(2));
    let proof = String::from_utf8(explain.stdout).unwrap();
    assert!(
        proof.contains("CAUSED BY: duration(") && proof.contains("GIVEN: compare("),
        "{proof}"
    );
    let preview = cli(&path, &["--fix", "--json"]);
    assert_eq!(
        preview.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&preview.stdout)
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
    let written = cli(&path, &["--fix", "--write", "--json"]);
    assert_eq!(written.status.code(), Some(0));
    let fixed: Value = serde_json::from_slice(&written.stdout).unwrap();
    assert_eq!(fixed["fix"]["changed"], true);
    assert!(fixed["fix"]["diff"]
        .as_str()
        .unwrap()
        .contains("+    lit: 10s"));
    assert_eq!(
        fixed["fix"]["semantic_diff"]["document_fields"],
        serde_json::json!(["trigger"])
    );
    let second = cli(&path, &["--fix", "--write", "--json"]);
    let second: Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(second["ok"], true);
    assert_eq!(second["fix"]["changed"], false);
    assert_eq!(second["fix"]["diff"], "");
}
#[test]
fn blueprint_v2_check_cli_refuses_ambiguous_and_authority_repairs_without_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("refusal.yaml");
    for node in [
        "  run: {lit: 'echo ready'}\n  with: {custom: {input: unknown}}\n",
        "  run: {lit: 'echo ready'}\n  caps: [net:example.org]\n",
    ] {
        let source = format!("schema_version: 2\nkind: pipeline\nname: refused\ntrigger: {{type: schedule, every: {{lit: 20s}}}}\nsteps:\n- id: emit\n{node}");
        std::fs::write(&path, &source).unwrap();
        let result = cli(&path, &["--fix", "--write", "--json"]);
        assert_eq!(result.status.code(), Some(2));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
        let value: Value = serde_json::from_slice(&result.stdout).unwrap();
        assert!(!value["fix"]["refusals"].as_array().unwrap().is_empty());
    }
}

#[test]
fn blueprint_v2_call_review_fix_calls_refuses_explicitly_without_false_c1_or_write() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("caller.yaml");
    let source = "schema_version: 2\nkind: pipeline\nname: caller\ntrigger: {type: schedule, every: {lit: 20s}}\nsteps:\n- id: invoke\n  call: {ref: 'mem:helper'}\n";
    std::fs::write(&path, source).unwrap();
    let output = cli(&path, &["--fix", "--write"]);
    assert_eq!(output.status.code(), Some(2));
    let message = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        message.contains("--fix") && message.contains("host"),
        "{message}"
    );
    assert!(!message.contains("C1"), "{message}");
    assert_eq!(std::fs::read_to_string(path).unwrap(), source);
}

#[test]
fn blueprint_v2_check_cli_never_overwrites_with_inadmissible_repair() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("pipeline.yaml");
    let source = "# keep author comments on refusal\nschema_version: 2\nkind: pipeline\nname: pipeline-repair\ntrigger: {type: schedule, every: {lit: 20s}}\nsteps:\n- id: decide\n  uses: ask\n  with: {question: {lit: 'Ready?'}}\n  buckets: [ready]\n";
    std::fs::write(&path, source).unwrap();
    let original = cli(&path, &["--json"]);
    assert_eq!(original.status.code(), Some(0));
    let result = cli(&path, &["--fix", "--write", "--json"]);
    assert_eq!(result.status.code(), Some(2));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
    let value: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["fix"]["changed"], false);
    assert!(value["fix"]["text"].is_null());
}
