//! Single seam for constructing role-carrying spawns. Every spawn edge routes
//! its role-identity and team-pairing inputs through here instead of adding a
//! second post-launch greeting.
//!
//! The two concerns:
//! - Role identity (system prompt) — `forming_identity` mints the placeholder
//!   `ManagerBootstrapInputs`; `forming_identity_in_team` binds an already
//!   reserved Team onto it while the plan stays unbound.
//! - Team coordination — the single launch payload names each counterpart
//!   before either provider process starts.

use crate::service::role_identity::ManagerBootstrapInputs;

/// Placeholder ids carried by a forming manager before a team/plan is bound.
pub(crate) const UNBOUND_TEAM: &str = "not yet established";
pub(crate) const UNBOUND_PLAN: &str = "no plan bound yet";
/// Display name for a reserved Team nobody has named yet.
pub(crate) const UNNAMED_TEAM: &str = "Unnamed Team";

/// The durable Team a forming manager is launched into: the exact id its tab
/// record carries, plus the name its startup contract renders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TeamIdentity {
    pub id: String,
    pub name: String,
}

impl TeamIdentity {
    pub(crate) fn new(id: impl Into<String>, name: Option<&str>) -> Self {
        Self {
            id: id.into(),
            name: name
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .unwrap_or(UNNAMED_TEAM)
                .to_string(),
        }
    }
}

/// Role-identity inputs for a manager spawned before its team/plan exist.
pub(crate) fn forming_identity(
    role: &str,
    role_kind: &str,
    project: String,
) -> ManagerBootstrapInputs {
    ManagerBootstrapInputs {
        role: role.into(),
        role_kind: role_kind.into(),
        team_id: UNBOUND_TEAM.into(),
        team_name: UNBOUND_TEAM.into(),
        plan_id: UNBOUND_PLAN.into(),
        project,
        coordination: None,
    }
}

/// Role-identity inputs for a manager launched into a Team that already exists,
/// while its plan is still created lazily on the first goal.
pub(crate) fn forming_identity_in_team(
    role: &str,
    role_kind: &str,
    project: String,
    team: &TeamIdentity,
) -> ManagerBootstrapInputs {
    ManagerBootstrapInputs {
        team_id: team.id.clone(),
        team_name: team.name.clone(),
        ..forming_identity(role, role_kind, project)
    }
}

/// A member of a freshly-formed pairing — the minimal identity another member
/// needs to reach it: display name (tab title), tab id, and human role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Counterpart {
    pub label: String,
    pub tab_id: String,
    pub role: String,
}

impl Counterpart {
    pub(crate) fn new(role: &str, label: impl Into<String>, tab_id: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            tab_id: tab_id.into(),
            role: role.into(),
        }
    }
}

/// The exact reach command a member uses to message its counterpart. `--tab`
/// accepts the tab id (see prompts/codeterm_rule.md).
pub(crate) fn reach_command(tab_id: &str) -> String {
    format!("codeterm send \"…\" --tab {tab_id}")
}

/// The body one member reads about its counterpart: names it, its tab id, and
/// the reach command, and forbids spawning a duplicate of an already-live pair.
pub(crate) fn introduction_body(me: &Counterpart, other: &Counterpart) -> String {
    format!(
        "You are the {my_role} of this team. Your {other_role} counterpart is {other_label} (tab {other_tab}) — reach it with `{reach}`. It already exists: coordinate through it, never spawn a duplicate.",
        my_role = me.role,
        other_role = other.role,
        other_label = other.label,
        other_tab = other.tab_id,
        reach = reach_command(&other.tab_id),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn orch() -> Counterpart {
        Counterpart::new("Orchestrator", "Kepler", "orch-tab-1111")
    }

    fn planner() -> Counterpart {
        Counterpart::new("Planner", "Euler", "planner-tab-2222")
    }

    // Byron regression: a formed pairing must yield BOTH directions, and each
    // body must name the counterpart's tab id AND the reach command. The
    #[test]
    fn pair_introductions_name_both_counterpart_tab_ids_and_reach_commands() {
        let (o, p) = (orch(), planner());
        let to_orch = introduction_body(&o, &p);
        let to_planner = introduction_body(&p, &o);

        // Orchestrator learns the planner's tab id + reach command.
        assert!(to_orch.contains(&p.tab_id), "names planner Tab id");
        assert!(to_orch.contains("Euler"), "names planner");
        assert!(
            to_orch.contains(&reach_command(&p.tab_id)),
            "carries the reach command for the planner"
        );

        // Planner learns the orchestrator's tab id + reach command.
        assert!(to_planner.contains(&o.tab_id), "names orch Tab id");
        assert!(to_planner.contains("Kepler"), "names orchestrator");
        assert!(
            to_planner.contains(&reach_command(&o.tab_id)),
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
    fn reach_command_uses_tab_flag_with_tab_id() {
        assert_eq!(
            reach_command("abc-123"),
            "codeterm send \"…\" --tab abc-123"
        );
    }

    #[test]
    fn forming_identity_carries_placeholders_not_fabricated_ids() {
        let id = forming_identity("Planner", "planner", "CodeTerm".into());
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

    #[test]
    fn team_identity_keeps_a_real_name_and_falls_back_when_unnamed() {
        assert_eq!(
            TeamIdentity::new("team_abcd1234", Some("Halley")).name,
            "Halley"
        );
        assert_eq!(TeamIdentity::new("team_abcd1234", None).name, UNNAMED_TEAM);
        assert_eq!(
            TeamIdentity::new("team_abcd1234", Some("   ")).name,
            UNNAMED_TEAM
        );
        assert_eq!(TeamIdentity::new("team_abcd1234", None).id, "team_abcd1234");
    }

    #[test]
    fn forming_identity_in_team_carries_the_team_and_leaves_the_plan_unbound() {
        let team = TeamIdentity::new("team_abcd1234", Some("Halley"));
        let id = forming_identity_in_team("Orchestrator", "orchestrator", "CodeTerm".into(), &team);

        assert_eq!(id.team_id, team.id);
        assert_eq!(id.team_name, team.name);
        assert_eq!(id.plan_id, UNBOUND_PLAN);
        let rendered = id.compose();
        assert!(rendered.contains(&team.id));
        assert!(rendered.contains(&team.name));
        assert!(rendered.contains(UNBOUND_PLAN));
        assert!(
            !rendered.contains(UNBOUND_TEAM),
            "a bound Team never renders the unbound placeholder"
        );
    }
}
