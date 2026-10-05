//! The immediate per-tool permission signal a hook-capable provider emits.
//! Vendors send `tool_name`, `tool_input` and `permission_suggestions[]` on
//! stdin; CodeTerm used to POST an empty body and throw all of it away. The
//! payload is kept verbatim as tool context and request identity; its
//! suggestions do not describe the rendered response choices.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRequestSignal {
    pub tool_name: String,
    pub tool_input: Value,
    pub permission_suggestions: Vec<Value>,
}

/// Parse a hook body into the signal. Absent `tool_name` means this is not a
/// per-tool permission report (e.g. the late `Notification` fallback), so the
/// caller keeps its existing screen-parsing path.
pub fn parse_permission_request(body: &Value) -> Option<PermissionRequestSignal> {
    let root = body
        .get("payload")
        .filter(|v| v.is_object())
        .unwrap_or(body);
    let tool_name = root
        .get("tool_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())?;
    Some(PermissionRequestSignal {
        tool_name: tool_name.to_string(),
        tool_input: root.get("tool_input").cloned().unwrap_or(Value::Null),
        permission_suggestions: root
            .get("permission_suggestions")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
    })
}

/// The binding nonce a hook body carries, if any. Scoping is enforced by the
/// caller against the tab's own launch nonce.
pub fn body_binding_nonce(body: &Value) -> Option<&str> {
    body.get("binding_nonce")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|nonce| !nonce.is_empty())
}

#[cfg(test)]
mod tests;
