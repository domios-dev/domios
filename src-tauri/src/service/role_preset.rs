//! One seam for the guidance a role kind carries. Manager kinds and Worker
//! subtypes resolve the same way, keyed by kind id and never a role literal.

use codeterm_types::agent::role_kind::{self, builtins};

use crate::multiplexer::types::AgentRole;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RolePreset {
    pub kind_id: String,
    pub label: String,
    pub guidance: String,
}

/// Bundled guidance for a builtin kind. `general_agent` is absent on purpose:
/// the machine agent composes its own instructions file.
pub(crate) fn builtin_guidance(kind_id: &str) -> Option<String> {
    match kind_id {
        builtins::ORCHESTRATOR => Some(crate::prompts::orchestrator_suffix()),
        builtins::PLANNER | builtins::WATCHER => Some(crate::prompts::planner_suffix()),
        builtins::WORKER => Some(crate::prompts::worker_suffix()),
        _ => None,
    }
}

/// Guidance for a manager bootstrap, keyed by kind instead of a role literal.
pub(crate) fn manager_guidance(kind_id: &str) -> String {
    builtin_guidance(kind_id).unwrap_or_else(crate::prompts::planner_suffix)
}

/// An overlay kind has no bundled text, so it inherits its structural base and
/// layers the supplied persona on top — the same composition managers get.
pub(crate) fn resolve(
    role: Option<AgentRole>,
    role_profile: Option<&str>,
    overlay_guidance: Option<&str>,
) -> Option<RolePreset> {
    let kind_id = role_kind::kind_id(role, role_profile)?.to_string();
    let label = role_kind::kind_label(role, role_profile)
        .map(str::to_string)
        .unwrap_or_else(|| kind_id.clone());
    let base = match builtin_guidance(&kind_id) {
        Some(guidance) => guidance,
        // Only a leaf overlay inherits a base; an unknown manager kind carries
        // no bundled authority text and must state its own.
        None if role == Some(AgentRole::Worker) => crate::prompts::worker_suffix(),
        None => String::new(),
    };
    let guidance = match overlay_guidance
        .map(str::trim)
        .filter(|guidance| !guidance.is_empty())
    {
        Some(overlay) if base.is_empty() => overlay.to_string(),
        Some(overlay) => format!("{base}\n\n{overlay}"),
        None => base,
    };
    Some(RolePreset {
        kind_id,
        label,
        guidance,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manager_guidance_is_keyed_by_kind_not_by_a_role_literal() {
        assert_eq!(
            manager_guidance(builtins::ORCHESTRATOR),
            crate::prompts::orchestrator_suffix()
        );
        assert_eq!(
            manager_guidance(builtins::PLANNER),
            crate::prompts::planner_suffix()
        );
        assert_eq!(
            manager_guidance(builtins::WATCHER),
            crate::prompts::planner_suffix(),
            "a managing leaf without bundled text keeps the planner baseline"
        );
    }

    #[test]
    fn a_worker_subtype_layers_its_task_persona_on_the_worker_base() {
        let preset = resolve(
            Some(AgentRole::Worker),
            Some("rust-platform-dev"),
            Some("You are Nadia."),
        )
        .expect("worker subtype preset");

        assert_eq!(preset.kind_id, "rust-platform-dev");
        assert_eq!(preset.label, "Worker", "an overlay kind keeps its base label");
        assert!(preset.guidance.starts_with(&crate::prompts::worker_suffix()));
        assert!(preset.guidance.ends_with("You are Nadia."));
    }

    #[test]
    fn a_plain_worker_resolves_the_bundled_worker_preset() {
        let preset = resolve(Some(AgentRole::Worker), None, None).expect("worker preset");

        assert_eq!(preset.kind_id, builtins::WORKER);
        assert_eq!(preset.label, "Worker");
        assert_eq!(preset.guidance, crate::prompts::worker_suffix());
    }

    #[test]
    fn manager_kinds_resolve_the_same_way_workers_do() {
        for (kind, label) in [
            (builtins::ORCHESTRATOR, "Orchestrator"),
            (builtins::PLANNER, "Planner"),
        ] {
            let preset =
                resolve(Some(AgentRole::Manager), Some(kind), None).expect("manager preset");
            assert_eq!(preset.kind_id, kind);
            assert_eq!(preset.label, label);
            assert_eq!(preset.guidance, builtin_guidance(kind).expect("bundled"));
        }
    }

    #[test]
    fn a_roleless_tab_resolves_no_preset() {
        assert!(resolve(None, Some(builtins::PLANNER), Some("persona")).is_none());
    }

    #[test]
    fn a_worker_subtype_cannot_borrow_a_manager_preset() {
        let preset = resolve(Some(AgentRole::Worker), Some(builtins::ORCHESTRATOR), None)
            .expect("worker preset");

        assert_eq!(preset.kind_id, builtins::WORKER);
        assert_eq!(preset.guidance, crate::prompts::worker_suffix());
    }
}
