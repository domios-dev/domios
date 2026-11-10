pub(super) const LEGACY_SESSION_ID_FIELD: &str = "claude_session_id";

pub(super) fn reject_legacy_session_id(value: &serde_json::Value) -> Result<(), String> {
    match value {
        serde_json::Value::Object(object) => {
            if object.get("type").and_then(serde_json::Value::as_str) == Some("Leaf")
                && object.contains_key(LEGACY_SESSION_ID_FIELD)
            {
                return Err(format!(
                    "unsupported legacy desk field {LEGACY_SESSION_ID_FIELD}; expected agent_session_id"
                ));
            }
            for key in ["workspaces", "tabs", "split_tree", "children"] {
                if let Some(value) = object.get(key) {
                    reject_legacy_session_id(value)?;
                }
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                reject_legacy_session_id(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}
