//! Single seam for constructing role-carrying spawns. Every spawn edge routes
//! its role-identity and team-pairing inputs through here instead of adding a
//! second post-launch greeting.
//!
//! The two concerns:
//! - Role identity (system prompt) — `forming_identity` mints the placeholder
//!   `ManagerBootstrapInputs` shared by the pair + standalone-planner edges.
//! - Team coordination — the single launch payload names each counterpart
//!   before either provider process starts.

use crate::service::role_identity::ManagerBootstrapInputs;

/// Placeholder ids carried by a forming manager before a team/plan is bound.
pub(crate) const UNBOUND_TEAM: &str = "not yet established";
pub(crate) const UNBOUND_PLAN: &str = "no plan bound yet";

/// Role-identity inputs for a manager spawned before its team/plan exist.
pub(crate) fn forming_identity(role: &str, project: String) -> ManagerBootstrapInputs {
    ManagerBootstrapInputs {
        role: role.into(),
        team_id: UNBOUND_TEAM.into(),
        team_name: UNBOUND_TEAM.into(),
        plan_id: UNBOUND_PLAN.into(),
        project,
        coordination: None,
    }
}

/// A member of a freshly-formed pairing — the minimal identity another member
/// needs to reach it: display name (tab title), pane id, and human role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Counterpart {
    pub label: String,
    pub pane_id: String,
    pub role: String,
}

impl Counterpart {
    pub(crate) fn new(role: &str, label: impl Into<String>, pane_id: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            pane_id: pane_id.into(),
            role: role.into(),
        }
    }
}

/// The exact reach command a member uses to message its counterpart. `--tab`
/// accepts the pane id (see prompts/codeterm_rule.md).
pub(crate) fn reach_command(pane_id: &str) -> String {
    format!("codeterm send \"…\" --tab {pane_id}")
}

/// The body one member reads about its counterpart: names it, its pane id, and
/// the reach command, and forbids spawning a duplicate of an already-live pair.
pub(crate) fn introduction_body(me: &Counterpart, other: &Counterpart) -> String {
    format!(
        "You are the {my_role} of this team. Your {other_role} counterpart is {other_label} (tab {other_pane}) — reach it with `{reach}`. It already exists: coordinate through it, never spawn a duplicate.",
        my_role = me.role,
        other_role = other.role,
        other_label = other.label,
        other_pane = other.pane_id,
        reach = reach_command(&other.pane_id),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn orch() -> Counterpart {
        Counterpart::new("Orchestrator", "Kepler", "orch-pane-1111")
    }

    fn planner() -> Counterpart {
        Counterpart::new("Planner", "Euler", "planner-pane-2222")
    }

    // Byron regression: a formed pairing must yield BOTH directions, and each
    // body must name the counterpart's pane id AND the reach command. The
    // pre-factory shape delivered only a one-directional display greeting that
    // named the counterpart by label with no pane id — the duplicate-spawn bug.
    #[test]
    fn pair_introductions_name_both_counterpart_pane_ids_and_reach_commands() {
        let (o, p) = (orch(), planner());
        let to_orch = introduction_body(&o, &p);
        let to_planner = introduction_body(&p, &o);

        // Orchestrator learns the planner's pane id + reach command.
        assert!(to_orch.contains(&p.pane_id), "names planner pane id");
        assert!(to_orch.contains("Euler"), "names planner");
        assert!(
            to_orch.contains(&reach_command(&p.pane_id)),
            "carries the reach command for the planner"
        );

        // Planner learns the orchestrator's pane id + reach command.
        assert!(to_planner.contains(&o.pane_id), "names orch pane id");
        assert!(to_planner.contains("Kepler"), "names orchestrator");
        assert!(
            to_planner.contains(&reach_command(&o.pane_id)),
            "carries the reach command for the orchestrator"
        );

        // Both are told not to duplicate the already-live counterpart.
        for body in [&to_orch, &to_planner] {
            assert!(
                body.contains("never spawn a duplicate"),
                "each member is warned against duplicating its pair"
            );
        }
    }

    #[test]
    fn reach_command_uses_tab_flag_with_pane_id() {
        assert_eq!(
            reach_command("abc-123"),
            "codeterm send \"…\" --tab abc-123"
        );
    }

    #[test]
    fn forming_identity_carries_placeholders_not_fabricated_ids() {
        let id = forming_identity("Planner", "CodeTerm".into());
        assert_eq!(id.role, "Planner");
        assert_eq!(id.project, "CodeTerm");
        assert_eq!(id.team_id, UNBOUND_TEAM);
        assert_eq!(id.plan_id, UNBOUND_PLAN);
        let rendered = id.compose();
        assert!(rendered.contains(UNBOUND_TEAM));
        assert!(rendered.contains(UNBOUND_PLAN));
        assert!(!rendered.contains("team_"));
        assert!(!rendered.contains("pl_"));
    }
}
