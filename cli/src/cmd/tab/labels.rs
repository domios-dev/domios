//! One label rule for every CLI tab surface.
//!
//! The agent IDENTITY is `provider:model`, suffixed by what was observed about
//! the process, and never carries the role. Role and mesh peer are separate
//! components, sourced the same way everywhere so `tab list` and `workspace map`
//! cannot disagree about the same tab. `tab list` brackets the identity alone;
//! `workspace map` brackets a detail list whose first component is the identity.
//! One rule for the identity, two layouts around it.

use codeterm_types::pty::{agent_bracket_label, AgentLiveness};
use serde_json::Value;

fn text<'a>(tab: &'a Value, key: &str) -> &'a str {
    tab.get(key).and_then(Value::as_str).unwrap_or("")
}

fn non_empty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn liveness_of(tab: &Value) -> AgentLiveness {
    match text(tab, "agent_liveness") {
        "live" => AgentLiveness::Live,
        "absent" => AgentLiveness::Absent,
        _ => AgentLiveness::Unobserved,
    }
}

/// The bracket contents for a tab row, or `None` when the tab declares no
/// provider. Prefers the label the daemon already composed; a projection that
/// predates the field is recomposed through the same shared rule, never a
/// second one.
pub fn agent_label(tab: &Value) -> Option<String> {
    if let Some(label) = non_empty(text(tab, "agent_label")) {
        return Some(label.to_string());
    }
    agent_bracket_label(
        non_empty(text(tab, "agent_type")),
        non_empty(text(tab, "agent_session_model")),
        liveness_of(tab),
    )
}

/// The tab's role, lowercased, from the more specific field first. `workspace
/// map` read `role_profile` and `tab list` read `role`, which is why the same
/// General Agent showed up as `general_agent` in one and `manager` in the other.
pub fn role_label(tab: &Value) -> String {
    ["role_profile", "role"]
        .into_iter()
        .filter_map(|key| non_empty(text(tab, key)))
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_bracket_is_provider_model_for_every_role_and_lifecycle_row() {
        let rows = [
            (
                json!({"agent_type": "claude", "agent_session_model": "opus",
                       "agent_liveness": "live", "role": "Manager",
                       "role_profile": "orchestrator"}),
                Some("claude:opus"),
                "orchestrator",
            ),
            (
                json!({"agent_type": "claude", "agent_session_model": "opus",
                       "agent_liveness": "live", "role": "Manager",
                       "role_profile": "planner"}),
                Some("claude:opus"),
                "planner",
            ),
            (
                json!({"agent_type": "codex", "agent_session_model": "gpt-5",
                       "agent_liveness": "live", "role": "Worker"}),
                Some("codex:gpt-5"),
                "worker",
            ),
            (
                json!({"agent_type": "claude", "agent_session_model": "opus",
                       "agent_liveness": "live", "role": "Manager",
                       "role_profile": "general_agent"}),
                Some("claude:opus"),
                "general_agent",
            ),
            (
                json!({"agent_type": "claude", "agent_session_model": "opus",
                       "agent_liveness": "unobserved"}),
                Some("claude:opus?"),
                "",
            ),
            (
                json!({"agent_type": "claude", "agent_session_model": "opus",
                       "agent_liveness": "absent", "role": "Manager"}),
                Some("claude:opus!"),
                "manager",
            ),
            (json!({"agent_liveness": "absent"}), None, ""),
        ];
        for (tab, expected_label, expected_role) in rows {
            assert_eq!(agent_label(&tab).as_deref(), expected_label, "{tab}");
            assert_eq!(role_label(&tab), expected_role, "{tab}");
        }
    }

    #[test]
    fn a_projection_without_the_daemon_label_recomposes_the_same_string() {
        let composed = json!({"agent_type": "claude", "agent_session_model": "opus",
                              "agent_liveness": "live"});
        let published = json!({"agent_type": "claude", "agent_session_model": "opus",
                               "agent_liveness": "live", "agent_label": "claude:opus"});
        assert_eq!(agent_label(&composed), agent_label(&published));
    }

    #[test]
    fn an_unknown_liveness_value_is_unobserved_not_live() {
        let tab = json!({"agent_type": "claude", "agent_session_model": "opus"});
        assert_eq!(agent_label(&tab).as_deref(), Some("claude:opus?"));
    }

    #[test]
    fn the_bracket_never_carries_a_role() {
        for role in [
            "manager",
            "worker",
            "general_agent",
            "orchestrator",
            "planner",
        ] {
            let tab = json!({"agent_type": "claude", "agent_session_model": "opus",
                             "agent_liveness": "live", "role_profile": role});
            assert!(!agent_label(&tab).unwrap().contains(role), "{role}");
        }
    }
}
