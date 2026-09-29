use serde_json::{json, Value};

use super::*;
use crate::cmd::print::{write_cli_error, CliError};

fn envelope_of(error: &CliError) -> Value {
    let mut out = Vec::new();
    let mut err = Vec::new();
    write_cli_error(true, error, &mut out, &mut err);
    assert!(err.is_empty(), "--json puts the whole failure on stdout");
    serde_json::from_slice(&out).expect("stdout carries exactly one JSON envelope")
}

#[test]
fn every_error_code_has_a_distinct_wire_name_and_a_hint() {
    let mut wire: Vec<&str> = ALL_ERROR_CODES.iter().map(|code| code.as_str()).collect();
    wire.sort_unstable();
    wire.dedup();
    assert_eq!(wire.len(), ALL_ERROR_CODES.len());
    for code in ALL_ERROR_CODES {
        assert!(!code.hint().trim().is_empty(), "{}", code.as_str());
        assert_eq!(ErrorCode::from_wire(code.as_str()), Some(code));
    }
}

#[test]
fn not_found_maps_to_code() {
    let missing: [(&str, Option<i32>, Option<Value>); 8] = [
        ("Tab not found: deadbeef", Some(-32602), None),
        (
            "one or more requested Tabs were not found",
            Some(-32603),
            None,
        ),
        ("Cron job not found: nope", Some(-32602), None),
        ("plugin 'nosuch' is not installed", None, None),
        ("channel 'qa' not registered", None, None),
        ("HTTP 404: anything at all", None, None),
        ("boom", None, Some(json!({ "http_status": 404 }))),
        ("boom", Some(-32603), Some(json!({ "code": "not_found" }))),
    ];
    for (message, rpc_code, data) in missing {
        assert_eq!(
            classify(message, rpc_code, data.as_ref()),
            ErrorCode::NotFound,
            "{message}"
        );
        let envelope = error_envelope(message, rpc_code, data.as_ref());
        assert_eq!(envelope["ok"], json!(false));
        assert_eq!(envelope["error"]["code"], "not_found");
        assert_eq!(envelope["error"]["message"], message);
        assert_eq!(envelope["error"]["rpc_code"], json!(rpc_code));
    }
}

#[test]
fn daemon_connection_failures_are_unavailable() {
    for message in [
        "failed to connect to local API: refused",
        "CodeTerm socket attempt failed: nothing listening; loopback HTTP attempt failed: x",
        "CodeTerm socket 'codeterm-dev.sock' is not live.",
    ] {
        assert_eq!(
            classify(message, None, None),
            ErrorCode::Unavailable,
            "{message}"
        );
    }
}

#[test]
fn structured_facts_beat_message_text() {
    assert_eq!(
        classify("Tab not found", None, Some(&json!({ "http_status": 409 }))),
        ErrorCode::Conflict
    );
    assert_eq!(
        classify("nope", Some(-32602), Some(&json!({ "code": "refused" }))),
        ErrorCode::Refused
    );
    assert_eq!(
        classify("Method not found", Some(-32601), None),
        ErrorCode::Unavailable
    );
    assert_eq!(
        classify("bad params", Some(-32602), None),
        ErrorCode::InvalidArgument
    );
    assert_eq!(classify("boom", Some(-32603), None), ErrorCode::Internal);
}

#[test]
fn a_declared_hint_replaces_the_generic_one() {
    let data = json!({ "retry_hint": "check the target before resending" });
    let envelope = error_envelope("refused", None, Some(&data));
    assert_eq!(
        envelope["error"]["hint"],
        "check the target before resending"
    );
    assert_eq!(
        error_envelope("refused", None, None)["error"]["hint"],
        ErrorCode::Internal.hint()
    );
}

#[test]
fn rendered_rpc_failure_text_recovers_its_code_and_drops_the_prefix() {
    let error = CliError::new("RPC error -32602: Tab not found: deadbeef");
    assert_eq!(error.code, Some(-32602));
    assert_eq!(error.message, "Tab not found: deadbeef");
    let envelope = envelope_of(&error);
    assert_eq!(envelope["error"]["code"], "not_found");
    assert_eq!(envelope["error"]["rpc_code"], -32602);
}

#[test]
fn rendered_http_failure_text_keeps_status_and_structured_body() {
    for text in [
        "HTTP 409: {\"error\":\"tab is busy\",\"refused\":true}",
        "HTTP 409 Conflict: {\"error\":\"tab is busy\",\"refused\":true}",
        "HTTP 409 — {\"error\":\"tab is busy\",\"refused\":true}",
    ] {
        let error = CliError::new(text);
        let data = error.data.as_ref().expect("http failures carry data");
        assert_eq!(data["http_status"], 409, "{text}");
        assert_eq!(data["refused"], true, "{text}");
        assert_eq!(error.message, "HTTP 409: tab is busy", "{text}");
        assert_eq!(envelope_of(&error)["error"]["code"], "conflict", "{text}");
    }
}

#[test]
fn http_failure_without_a_body_still_classifies_by_status() {
    let error = CliError::http(404, "");
    assert_eq!(error.message, "HTTP 404");
    assert_eq!(envelope_of(&error)["error"]["code"], "not_found");
    let error = CliError::http(503, "");
    assert_eq!(envelope_of(&error)["error"]["code"], "unavailable");
}

#[test]
fn usage_failures_are_invalid_argument_whatever_their_text() {
    use clap::Parser;

    let clap_error = crate::args::cli::Cli::try_parse_from(["codeterm", "mem", "get"])
        .err()
        .expect("mem get requires an id");
    let error = CliError::usage(&clap_error);
    assert!(!error.message.trim().is_empty());
    let envelope = envelope_of(&error);
    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_eq!(envelope["ok"], json!(false));
}

#[test]
fn json_error_envelope_shape_is_shared_by_every_error_path() {
    let envelope = error_envelope("x", None, None);
    let mut keys: Vec<&str> = envelope
        .as_object()
        .expect("envelope is an object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["data", "error", "hints", "id", "kind", "ok"]);
    let mut error_keys: Vec<&str> = envelope["error"]
        .as_object()
        .expect("error is an object")
        .keys()
        .map(String::as_str)
        .collect();
    error_keys.sort_unstable();
    assert_eq!(error_keys, ["code", "hint", "message", "rpc_code"]);
}

#[test]
fn no_command_prints_an_error_and_exits_by_hand() {
    const BY_HAND: &str = concat!("eprintln!(\"", "Error:");
    const OWN_REPORT: &[&str] = &["bridge/migrate.rs"];
    let mut pending = vec![std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
    let mut offenders = Vec::new();
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).expect("read source dir").flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let name = path.to_string_lossy().replace('\\', "/");
            let is_test = name.ends_with("tests.rs")
                || name.ends_with("_tests.rs")
                || name.contains("/tests/");
            let exempt = OWN_REPORT.iter().any(|own| name.ends_with(own));
            if !name.ends_with(".rs") || is_test || exempt {
                continue;
            }
            if std::fs::read_to_string(&path)
                .unwrap_or_default()
                .contains(BY_HAND)
            {
                offenders.push(name);
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "failures must leave through cmd::print::fail*, which honours --json: {offenders:?}"
    );
}
