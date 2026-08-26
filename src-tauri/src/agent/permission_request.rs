//! The immediate per-tool permission signal a hook-capable provider emits.
//! Vendors send `tool_name`, `tool_input` and `permission_suggestions[]` on
//! stdin; CodeTerm used to POST an empty body and throw all of it away. The
//! payload is kept verbatim here so nothing downstream has to re-scrape the
//! screen for what the provider already told us.

use codeterm_types::pty::{PendingPrompt, PromptOption};
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

impl PermissionRequestSignal {
    /// Render the vendor's own options as a surfaced prompt. Labels come from
    /// the suggestion payload, never from the screen. `None` when the vendor
    /// sent no suggestions — the signal is still recorded verbatim.
    pub fn to_pending_prompt(&self) -> Option<PendingPrompt> {
        let options: Vec<PromptOption> = self
            .permission_suggestions
            .iter()
            .enumerate()
            .filter_map(|(index, suggestion)| {
                suggestion_label(suggestion).map(|label| PromptOption {
                    label,
                    value: (index + 1).to_string(),
                })
            })
            .collect();
        if options.is_empty() {
            return None;
        }
        Some(PendingPrompt {
            question: self.tool_name.clone(),
            options,
            sections: None,
            multi_select: None,
            submit_available: None,
            selected_values: None,
        })
    }
}

/// Vendors have shipped several suggestion shapes; take the first
/// human-readable string rather than pinning one schema.
fn suggestion_label(suggestion: &Value) -> Option<String> {
    if let Some(text) = suggestion.as_str() {
        let text = text.trim();
        return (!text.is_empty()).then(|| text.to_string());
    }
    for key in ["label", "title", "description", "behavior", "type", "mode"] {
        if let Some(text) = suggestion.get(key).and_then(Value::as_str) {
            let text = text.trim();
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests;
