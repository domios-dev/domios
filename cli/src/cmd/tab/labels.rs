//! One label rule for every CLI tab surface.
//!
//! The agent IDENTITY is the short model name when a catalogue claims the id,
//! the provider when none does, suffixed by what was observed about the
//! process, and never the role. Role and mesh peer are separate
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
        non_empty(text(tab, "agent_session_model"))
            .or_else(|| non_empty(text(tab, "agent_provider_resolved_model"))),
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

    /// The bracket is the model alone when a catalogue claims it, the provider
    /// when none does, and never the two joined.
    #[test]
    fn the_bracket_is_the_model_alone_for_every_role_and_lifecycle_row() {
        let rows = [
            (
                json!({"agent_type": "claude", "agent_session_model": "opus",
                       "agent_liveness": "live", "role": "Manager",
                       "role_profile": "orchestrator"}),
                Some("opus"),
                "orchestrator",
            ),
            (
                json!({"agent_type": "claude", "agent_session_model": "claude-sonnet-4-5-20250929",
                       "agent_liveness": "live", "role": "Manager",
                       "role_profile": "planner"}),
                Some("sonnet"),
                "planner",
            ),
            // codex, cursor and opencode never report a session model; the
            // requested model is still the identity, liveness carries the truth.
            (
                json!({"agent_type": "codex", "agent_provider_resolved_model": "gpt-5.5",
                       "agent_liveness": "absent", "role": "Worker"}),
                Some("gpt 5.5!"),
                "worker",
            ),
            (
                json!({"agent_type": "codex", "agent_session_model": "gpt-5.6-sol",
                       "agent_liveness": "live", "role": "Worker"}),
                Some("5.6-sol"),
                "worker",
            ),
            (
                json!({"agent_type": "codex", "agent_session_model": "gpt-5.3-codex-spark",
                       "agent_liveness": "unobserved", "role": "Worker"}),
                Some("5.3-spark?"),
                "worker",
            ),
            (
                json!({"agent_type": "claude", "agent_session_model": "opus",
                       "agent_liveness": "live", "role": "Manager",
                       "role_profile": "general_agent"}),
                Some("opus"),
                "general_agent",
            ),
            (
                json!({"agent_type": "claude", "agent_session_model": "opus",
                       "agent_liveness": "unobserved"}),
                Some("opus?"),
                "",
            ),
            (
                json!({"agent_type": "claude", "agent_session_model": "opus",
                       "agent_liveness": "absent", "role": "Manager"}),
                Some("opus!"),
                "manager",
            ),
            // no model observed: the provider stands in, at every liveness
            (
                json!({"agent_type": "codex", "agent_liveness": "live"}),
                Some("codex"),
                "",
            ),
            (
                json!({"agent_type": "codex", "agent_liveness": "unobserved"}),
                Some("codex?"),
                "",
            ),
            (
                json!({"agent_type": "codex", "agent_liveness": "absent"}),
                Some("codex!"),
                "",
            ),
            // a model no catalogue claims: the provider stands in
            (
                json!({"agent_type": "lmstudio", "agent_session_model": "nvidia/nemotron-3-nano",
                       "agent_liveness": "live"}),
                Some("lmstudio"),
                "",
            ),
            (
                json!({"agent_type": "lmstudio", "agent_session_model": "some-local-gguf",
                       "agent_liveness": "absent", "role": "Worker"}),
                Some("lmstudio!"),
                "worker",
            ),
            (json!({"agent_liveness": "absent"}), None, ""),
        ];
        for (tab, expected_label, expected_role) in rows {
            assert_eq!(agent_label(&tab).as_deref(), expected_label, "{tab}");
            assert_eq!(role_label(&tab), expected_role, "{tab}");
        }
    }

    /// Choosing the provider is a rendering decision. The projection keeps the
    /// model it was published with.
    #[test]
    fn the_provider_fallback_leaves_the_projected_model_in_place() {
        let tab = json!({"agent_type": "lmstudio", "agent_session_model": "some-local-gguf",
                         "agent_liveness": "live"});
        assert_eq!(agent_label(&tab).as_deref(), Some("lmstudio"));
        assert_eq!(
            tab.get("agent_session_model").and_then(Value::as_str),
            Some("some-local-gguf")
        );
    }

    #[test]
    fn a_recognised_model_never_carries_its_provider() {
        for (provider, model) in [("claude", "claude-opus-5"), ("codex", "gpt-5.6-terra")] {
            for liveness in ["live", "unobserved", "absent"] {
                let tab = json!({"agent_type": provider, "agent_session_model": model,
                                 "agent_liveness": liveness});
                let label = agent_label(&tab).expect("a declared provider brackets");
                assert!(
                    !label.contains(':') && !label.contains(provider) && !label.contains(model),
                    "{provider}/{model} at {liveness} rendered {label}"
                );
            }
        }
    }

    #[test]
    fn a_projection_without_the_daemon_label_recomposes_the_same_string() {
        let composed = json!({"agent_type": "claude", "agent_session_model": "opus",
                              "agent_liveness": "live"});
        let published = json!({"agent_type": "claude", "agent_session_model": "opus",
                               "agent_liveness": "live", "agent_label": "opus"});
        assert_eq!(agent_label(&composed), agent_label(&published));
    }

    #[test]
    fn an_unknown_liveness_value_is_unobserved_not_live() {
        let tab = json!({"agent_type": "claude", "agent_session_model": "opus"});
        assert_eq!(agent_label(&tab).as_deref(), Some("opus?"));
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
