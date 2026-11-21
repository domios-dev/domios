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
    let bounded = bound_leaf(leaf).unwrap();
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
fn oversized_worker_persona_and_chat_backend_prompts_are_not_partially_saved() {
    for leaf in [
        json!({"role":"Worker", "agent_identity":{"restorable_launch":{
            "identity_instructions":"x".repeat(MAX_DESK_LEAF_BYTES)
        }}}),
        json!({"chatbackend_plugin_id":"fixture", "agent_identity":{"agent_runtime_config":{
            "systemPrompt":"x".repeat(MAX_DESK_LEAF_BYTES)
        }}}),
    ] {
        let bytes = serde_json::to_vec(&leaf).unwrap().len();
        assert_eq!(
            bound_leaf(leaf),
            Err(OversizedLayout {
                fields: "desk leaf fields",
                bytes,
            })
        );
    }
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
    let bounded = bound_leaf(leaf).unwrap();
    assert_eq!(
        bounded["agent_identity"]["agent_runtime_config"],
        json!({"model": "fixture"})
    );
    assert_eq!(
        bounded["agent_identity"]["restorable_launch"],
        json!({"account_id": "fixture"})
    );
}
