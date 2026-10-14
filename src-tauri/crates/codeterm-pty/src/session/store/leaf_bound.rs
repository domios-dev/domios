use codeterm_storage::MAX_DESK_LEAF_BYTES;
use serde_json::Value;

const DERIVED_PROMPT: &str = "/agent_identity/agent_runtime_config";
const SAVED_LAUNCH: &str = "/agent_identity/restorable_launch";

fn encoded_len(value: &Value) -> usize {
    serde_json::to_vec(value).map_or(usize::MAX, |bytes| bytes.len())
}

// The composed runtime prompt is rebuilt from role and saved identity on restore,
// so dropping it keeps one oversized tab from refusing the whole desk commit.
pub(super) fn bound_leaf(tab_id: &str, mut leaf: Value) -> Value {
    if encoded_len(&leaf) <= MAX_DESK_LEAF_BYTES {
        return leaf;
    }
    let dropped = leaf
        .pointer_mut(DERIVED_PROMPT)
        .and_then(Value::as_object_mut)
        .and_then(|config| config.remove("systemPrompt"))
        .is_some();
    // Only a Worker's saved launch identity carries a persona; others are rebuilt from role.
    let identity_dropped = encoded_len(&leaf) > MAX_DESK_LEAF_BYTES
        && leaf.get("role").and_then(Value::as_str) != Some("Worker")
        && leaf
            .pointer_mut(SAVED_LAUNCH)
            .and_then(Value::as_object_mut)
            .and_then(|launch| launch.remove("identity_instructions"))
            .is_some();
    log::warn!(
        "[desk] tab={tab_id} leaf exceeds {MAX_DESK_LEAF_BYTES} bytes; dropped prompt={dropped} identity={identity_dropped} remaining={}",
        encoded_len(&leaf)
    );
    leaf
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
