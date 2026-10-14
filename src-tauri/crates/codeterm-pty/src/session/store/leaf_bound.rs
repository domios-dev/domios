use codeterm_storage::MAX_DESK_LEAF_BYTES;
use serde_json::Value;

const DERIVED_PROMPT: &str = "/agent_identity/agent_runtime_config";

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
    log::warn!(
        "[desk] tab={tab_id} leaf exceeds {MAX_DESK_LEAF_BYTES} bytes; derived runtime prompt dropped={dropped} remaining={}",
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
}
