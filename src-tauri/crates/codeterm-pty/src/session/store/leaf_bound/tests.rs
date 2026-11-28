use super::*;
use serde_json::json;

#[test]
fn general_agent_prompt_is_rebuilt_from_role_even_when_it_fits() {
    let leaf = json!({
        "role": "Manager",
        "role_profile": codeterm_types::agent::role_kind::builtins::GENERAL_AGENT,
        "agent_identity": {
            "agent_runtime_config": {"systemPrompt": "opaque", "model": "fixture"},
            "restorable_launch": {"identity_instructions": "opaque", "account_id": "fixture"}
        }
    });
    let bounded = bound_leaf(leaf).unwrap().values;
    assert_eq!(
        bounded["agent_identity"]["agent_runtime_config"],
        json!({"model": "fixture"})
    );
    assert_eq!(
        bounded["agent_identity"]["restorable_launch"],
        json!({"account_id": "fixture"})
    );
}

#[test]
fn oversized_pty_worker_persona_is_not_partially_saved() {
    let leaf = json!({"role":"Worker", "agent_identity":{"restorable_launch":{
        "identity_instructions":"x".repeat(MAX_DESK_LEAF_BYTES)
    }}});
    let bytes = serde_json::to_vec(&leaf).unwrap().len();
    assert_eq!(
        bound_leaf(leaf),
        Err(OversizedLayout {
            fields: "desk leaf fields",
            bytes,
            limit: MAX_DESK_LEAF_BYTES,
        })
    );
}

#[test]
fn oversized_chat_backend_prompt_spills_whole_when_provenance_does_not_match() {
    let prompt = "ü\n".repeat(MAX_DESK_LEAF_BYTES);
    let identity = "opaque-persona";
    let leaf = json!({
        "chatbackend_plugin_id": "fixture",
        "role": "Worker",
        "agent_identity": {
            "agent_runtime_config": {"systemPrompt": prompt, "model": "fixture"},
            "restorable_launch": {
                "identity_instructions": identity,
                "runtime_prompt_source": {"regenerable": RuntimePromptSource::fingerprint("different")}
            }
        }
    });
    let bounded = bound_leaf(leaf).unwrap();
    bounded.validate().unwrap();
    assert_eq!(
        bounded.values["agent_identity"]["agent_runtime_config"],
        json!({"model": "fixture"})
    );
    assert_eq!(
        bounded.values["agent_identity"]["restorable_launch"],
        json!({})
    );
    assert_eq!(
        bounded.prompt.system_prompt.as_deref(),
        Some(prompt.as_str())
    );
    assert_eq!(
        bounded.prompt.identity_instructions.as_deref(),
        Some(identity)
    );
}

#[test]
fn oversized_manager_prompt_and_launch_instructions_can_both_be_rebuilt() {
    let leaf = json!({
        "role": "Manager",
        "role_profile": codeterm_types::agent::role_kind::builtins::ORCHESTRATOR,
        "agent_identity": {
            "agent_runtime_config": {"systemPrompt": "x".repeat(MAX_DESK_LEAF_BYTES / 2), "model": "fixture"},
            "restorable_launch": {"identity_instructions": "y".repeat(MAX_DESK_LEAF_BYTES), "account_id": "fixture"}
        }
    });
    let bounded = bound_leaf(leaf).unwrap().values;
    assert_eq!(
        bounded["agent_identity"]["agent_runtime_config"],
        json!({"model": "fixture"})
    );
    assert_eq!(
        bounded["agent_identity"]["restorable_launch"],
        json!({"account_id": "fixture"})
    );
}
