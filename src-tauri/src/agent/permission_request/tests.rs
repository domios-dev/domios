use super::*;
use crate::agent::prompt::{PromptDecision, PromptDetector};
use serde_json::json;

fn vendor_body() -> serde_json::Value {
    json!({
        "hook_event_name": "PermissionRequest",
        "tool_name": "Bash",
        "tool_input": {
            "command": "rm -rf ./build",
            "description": "clear the build dir",
            "timeout": 120000
        },
        "permission_suggestions": [
            {"type": "addRules", "behavior": "allow", "label": "Allow Bash(rm:*)"},
            {"type": "addRules", "behavior": "deny", "label": "Deny once"}
        ]
    })
}

#[test]
fn permission_request_payload_round_trips_every_supplied_field() {
    let body = vendor_body();
    let signal = parse_permission_request(&body).expect("vendor payload parses");

    assert_eq!(signal.tool_name, "Bash");
    // tool_input is preserved verbatim, not flattened or summarized.
    assert_eq!(signal.tool_input, body["tool_input"]);
    assert_eq!(
        signal.tool_input["command"].as_str(),
        Some("rm -rf ./build")
    );
    assert_eq!(signal.tool_input["timeout"].as_u64(), Some(120000));
    assert_eq!(
        signal.permission_suggestions,
        body["permission_suggestions"].as_array().unwrap().clone()
    );
}

#[test]
fn permission_request_payload_survives_the_detector_round_trip() {
    let body = vendor_body();
    let signal = parse_permission_request(&body).expect("vendor payload parses");
    let mut detector = PromptDetector::new();

    let decision = detector.hook_detected_permission_request("tab-1", signal);
    assert!(matches!(decision, PromptDecision::NewPrompt { .. }));

    let stored = detector
        .permission_request("tab-1")
        .expect("signal is retained, not discarded");
    assert_eq!(stored.tool_name, body["tool_name"].as_str().unwrap());
    assert_eq!(stored.tool_input, body["tool_input"]);
    assert_eq!(
        stored.permission_suggestions,
        body["permission_suggestions"].as_array().unwrap().clone()
    );
}

#[test]
fn permission_request_surfaces_the_vendor_supplied_options() {
    let signal = parse_permission_request(&vendor_body()).expect("parses");
    let prompt = signal
        .to_pending_prompt()
        .expect("suggestions become options");
    assert_eq!(prompt.question, "Bash");
    let labels: Vec<&str> = prompt.options.iter().map(|o| o.label.as_str()).collect();
    assert_eq!(labels, vec!["Allow Bash(rm:*)", "Deny once"]);
    let values: Vec<&str> = prompt.options.iter().map(|o| o.value.as_str()).collect();
    assert_eq!(values, vec!["1", "2"]);
}

#[test]
fn permission_request_empty_body_is_not_a_permission_signal() {
    // The exact regression: an empty body carries nothing to ingest, so the
    // caller must fall through to its existing path rather than land a blank.
    assert!(parse_permission_request(&json!({})).is_none());
    assert!(parse_permission_request(&serde_json::Value::Null).is_none());
    assert!(parse_permission_request(&json!({"tool_name": "   "})).is_none());
}

#[test]
fn permission_request_without_suggestions_still_records_the_payload() {
    let body = json!({"tool_name": "Edit", "tool_input": {"file_path": "a.rs"}});
    let signal = parse_permission_request(&body).expect("parses");
    assert!(signal.permission_suggestions.is_empty());
    assert!(signal.to_pending_prompt().is_none());

    let mut detector = PromptDetector::new();
    let decision = detector.hook_detected_permission_request("tab-2", signal);
    assert!(matches!(decision, PromptDecision::None));
    let stored = detector
        .permission_request("tab-2")
        .expect("still recorded");
    assert_eq!(stored.tool_input, body["tool_input"]);
}

#[test]
fn permission_request_reads_a_nested_payload_envelope() {
    let signal = parse_permission_request(&json!({
        "payload": {"tool_name": "Write", "tool_input": {"content": "x"}}
    }))
    .expect("parses");
    assert_eq!(signal.tool_name, "Write");
    assert_eq!(signal.tool_input["content"].as_str(), Some("x"));
}

#[test]
fn permission_request_reads_the_binding_nonce_from_the_body() {
    assert_eq!(
        body_binding_nonce(&json!({"binding_nonce": "nonce-1"})),
        Some("nonce-1")
    );
    assert_eq!(body_binding_nonce(&json!({"binding_nonce": "  "})), None);
    assert_eq!(body_binding_nonce(&json!({})), None);
}

#[test]
fn permission_request_clearing_a_tab_drops_the_stored_payload() {
    let signal = parse_permission_request(&vendor_body()).expect("parses");
    let mut detector = PromptDetector::new();
    detector.hook_detected_permission_request("tab-3", signal.clone());
    detector.clear("tab-3");
    assert!(detector.permission_request("tab-3").is_none());

    detector.hook_detected_permission_request("tab-4", signal);
    detector.remove_tab("tab-4");
    assert!(detector.permission_request("tab-4").is_none());
}
