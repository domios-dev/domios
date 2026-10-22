use super::*;

#[test]
fn oversized_warning_is_once_per_tab_and_changed_state() {
    let mut states = HashMap::new();
    assert!(claim_warning(&mut states, "a", Some(1)));
    for _ in 0..140 {
        assert!(!claim_warning(&mut states, "a", Some(1)));
    }
    assert!(claim_warning(&mut states, "b", Some(1)));
    assert!(claim_warning(&mut states, "a", Some(2)));
    assert!(!claim_warning(&mut states, "a", Some(2)));
    assert!(!claim_warning(&mut states, "a", None));
    assert!(claim_warning(&mut states, "a", Some(2)));
}

#[test]
fn removed_string_byte_accounting_matches_json_with_escapes_and_unicode() {
    for prompt in ["opaque", "\"\\\n\r\t\u{0008}\u{000c}\u{0000}\u{001f}é🙂"] {
        for sibling in [false, true] {
            let mut value = serde_json::json!({"config":{"systemPrompt":prompt}});
            if sibling {
                value["config"]["model"] = serde_json::json!("opaque");
            }
            let before = encoded_len(&value);
            let removed = remove_field(&mut value, "/config", "systemPrompt").unwrap();
            assert_eq!(before - removed, encoded_len(&value));
        }
    }
}

#[test]
fn leaf_bound_threshold_uses_remaining_bytes_after_prompt_removal() {
    let mut value = launch_leaf("Manager");
    value["agent_identity"]["restorable_launch"]["identity_instructions"] =
        serde_json::json!("y".repeat(MAX_DESK_LEAF_BYTES / 2));
    value["agent_identity"]["agent_runtime_config"]["systemPrompt"] =
        serde_json::json!("\n".repeat(MAX_DESK_LEAF_BYTES));
    let bounded = bound_leaf("remaining-threshold", value);
    assert!(encoded_len(&bounded) <= MAX_DESK_LEAF_BYTES);
    assert!(bounded
        .pointer("/agent_identity/restorable_launch/identity_instructions")
        .is_some());
}
use serde_json::json;

fn leaf(prompt_bytes: usize) -> Value {
    json!({"title":"opaque","agent_identity":{"agent_runtime_config":{
            "systemPrompt":"x".repeat(prompt_bytes),"model":"opaque-model"}}})
}

#[test]
fn leaf_within_bound_keeps_its_runtime_prompt() {
    let value = leaf(1024);
    assert_eq!(bound_leaf("tab", value.clone()), value);
}

#[test]
fn oversized_leaf_drops_only_the_derived_runtime_prompt() {
    let bounded = bound_leaf("tab", leaf(MAX_DESK_LEAF_BYTES));
    assert!(encoded_len(&bounded) <= MAX_DESK_LEAF_BYTES);
    assert!(bounded
        .pointer("/agent_identity/agent_runtime_config/systemPrompt")
        .is_none());
    assert_eq!(
        bounded.pointer("/agent_identity/agent_runtime_config/model"),
        Some(&json!("opaque-model"))
    );
    assert_eq!(bounded["title"], "opaque");
}

fn launch_leaf(role: &str) -> Value {
    json!({"role":role,"agent_identity":{
            "agent_runtime_config":{"systemPrompt":"x".repeat(MAX_DESK_LEAF_BYTES / 2)},
            "restorable_launch":{"identity_instructions":"y".repeat(MAX_DESK_LEAF_BYTES)}}})
}

#[test]
fn oversized_manager_leaf_drops_its_role_derived_launch_identity() {
    let bounded = bound_leaf("tab", launch_leaf("Manager"));
    assert!(encoded_len(&bounded) <= MAX_DESK_LEAF_BYTES);
    assert!(bounded
        .pointer("/agent_identity/restorable_launch/identity_instructions")
        .is_none());
}

#[test]
fn oversized_worker_leaf_keeps_its_persona_identity() {
    let bounded = bound_leaf("tab", launch_leaf("Worker"));
    assert!(bounded
        .pointer("/agent_identity/restorable_launch/identity_instructions")
        .is_some());
}
