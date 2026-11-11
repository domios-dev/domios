use std::path::{Path, PathBuf};

use codeterm_team_package::{check_path, CheckOptions};
use serde::Deserialize;

use super::run;

#[derive(Deserialize)]
struct Cases {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    path: String,
    valid: bool,
}

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/corpus")
}

fn invoke(args: &[&str]) -> (i32, String, String) {
    let args: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run(&args, &mut out, &mut err);
    let text = |bytes: Vec<u8>| String::from_utf8(bytes).expect("UTF-8 output");
    (code, text(out), text(err))
}

// `team check` serializes `check_path(path, CheckOptions::domios()).report` and exits 1 when invalid.
#[test]
fn json_report_and_exit_code_match_team_check_on_the_corpus() {
    let cases: Cases =
        serde_json::from_slice(&std::fs::read(corpus().join("cases.json")).expect("corpus cases"))
            .expect("cases.json");
    for case in cases.cases {
        let path = corpus().join(&case.path);
        let label = path.to_string_lossy().to_string();
        let (code, out, err) = invoke(&[&label, "--host-profile", "domios", "--json"]);
        let team_check =
            serde_json::to_value(check_path(&path, &CheckOptions::domios()).report).unwrap();
        let printed: serde_json::Value = serde_json::from_str(&out).expect("one JSON document");
        assert_eq!(printed, team_check, "{}", case.path);
        assert_eq!(code, if case.valid { 0 } else { 1 }, "{}: {err}", case.path);
    }
}

#[test]
fn human_output_lists_each_diagnostic_and_the_verdict() {
    let path = corpus().join("invalid/removed-kind.team.yaml");
    let label = path.to_string_lossy().to_string();
    let (code, out, _) = invoke(&[&label]);
    assert_eq!(code, 1);
    assert!(
        out.contains(&format!(
            "{label}:6:5: error[removed_field] managers.orchestrator.kind:"
        )),
        "{out}"
    );
    assert!(
        out.ends_with("invalid dev.example/probe@1.0.0: 1 error(s), 0 warning(s)\n"),
        "{out}"
    );
}

#[test]
fn usage_and_io_failures_exit_two() {
    let missing = corpus().join("does-not-exist.team.yaml");
    let missing = missing.to_string_lossy().to_string();
    for args in [
        vec![],
        vec!["a.team.yaml", "--host-profile", "other"],
        vec!["a.team.yaml", "--strict"],
        vec!["schema"],
        vec!["schema", "--spec-version", "2026-1-01"],
        vec![missing.as_str(), "--json"],
    ] {
        assert_eq!(invoke(&args).0, 2, "{args:?}");
    }
}

#[test]
fn schema_subcommand_writes_the_identified_schema_with_one_trailing_newline() {
    let (code, out, _) = invoke(&["schema", "--spec-version", "2026-11-01"]);
    assert_eq!(code, 0);
    assert!(out.ends_with("}\n") && !out.ends_with("\n\n"), "{out}");
    let schema: serde_json::Value = serde_json::from_str(&out).expect("schema JSON");
    assert_eq!(
        schema["$id"],
        "https://atp.domios.dev/schema/2026-11-01/team.schema.json"
    );
    assert!(schema["$schema"].is_string());
}
