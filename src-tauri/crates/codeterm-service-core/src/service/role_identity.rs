#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RoleIdentityContext<'a> {
    pub role: &'a str,
    pub team_id: &'a str,
    pub team_name: &'a str,
    pub plan_id: &'a str,
    pub project: &'a str,
    pub role_guidance: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagerBootstrapInputs {
    /// Display label for the manager kind (e.g. "Orchestrator", "Planner").
    pub role: String,
    /// Role kind id — the preset key. Guidance is resolved from this, never
    /// from the display label.
    pub role_kind: String,
    pub team_id: String,
    pub team_name: String,
    pub plan_id: String,
    pub project: String,
    pub coordination: Option<String>,
}

impl ManagerBootstrapInputs {
    pub fn compose(&self) -> String {
        let role_guidance = crate::service::persona::guidance_for(
            crate::service::persona::Occasion::ManagerBootstrap,
            &crate::service::team_roles::registrations(Some(&self.team_id)),
            Some(crate::multiplexer::types::AgentRole::Manager),
            Some(&self.role_kind),
            None,
        );
        let identity = compose_role_identity(RoleIdentityContext {
            role: &self.role,
            team_id: &self.team_id,
            team_name: &self.team_name,
            plan_id: &self.plan_id,
            project: &self.project,
            role_guidance: &role_guidance,
        });
        match self.coordination.as_deref() {
            Some(coordination) => format!("{identity}\n\n{coordination}"),
            None => identity,
        }
    }
}

pub(crate) fn compose_role_identity(context: RoleIdentityContext<'_>) -> String {
    let template = crate::prompts::MANAGER_BOOTSTRAP_TEMPLATE.replace("\r\n", "\n");
    codeterm_types::agent::role_identity::render(
        codeterm_types::agent::role_identity::RoleIdentity::Manager {
            template: &template,
            role: context.role,
            team_id: context.team_id,
            team_name: context.team_name,
            plan_id: context.plan_id,
            project: context.project,
            role_guidance: context.role_guidance,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn golden(text: &str) -> String {
        text.replace("\r\n", "\n")
    }

    #[test]
    fn manager_bootstrap_embeds_the_matching_canonical_manual() {
        for (role, role_kind, manual) in [
            (
                "Orchestrator",
                "orchestrator",
                crate::service::builtin_roles::builtin_prose("orchestrator").unwrap(),
            ),
            (
                "Planner",
                "planner",
                crate::service::builtin_roles::builtin_prose("planner").unwrap(),
            ),
        ] {
            let rendered = ManagerBootstrapInputs {
                role: role.into(),
                role_kind: role_kind.into(),
                team_id: "team-1".into(),
                team_name: "Team".into(),
                plan_id: "plan-1".into(),
                project: "CodeTerm".into(),
                coordination: None,
            }
            .compose();

            assert_eq!(rendered.matches(manual.trim()).count(), 1);
        }
    }

    #[test]
    fn counterpart_coordination_is_part_of_the_single_bootstrap_payload() {
        let coordination = "Your Planner counterpart is Euler (tab planner-1).";
        let mut inputs = ManagerBootstrapInputs {
            role: "Orchestrator".into(),
            role_kind: "orchestrator".into(),
            team_id: "not yet established".into(),
            team_name: "not yet established".into(),
            plan_id: "no plan bound yet".into(),
            project: "CodeTerm".into(),
            coordination: Some(coordination.into()),
        };
        let rendered = inputs.compose();
        assert_eq!(rendered.matches(coordination).count(), 1);
        inputs.coordination = None;
        assert!(!inputs.compose().contains(coordination));
    }

    #[test]
    fn renderer_never_reprocesses_placeholder_literals_in_values() {
        let rendered = compose_role_identity(RoleIdentityContext {
            role: "{plan_id}",
            team_id: "team-{role_guidance}",
            team_name: "{plan_id}",
            plan_id: "plan-{role}",
            project: "{role_guidance}",
            role_guidance: "keep {plan_id}",
        });
        assert!(rendered.contains("team-{role_guidance}"));
        assert!(rendered.contains("plan-{role}"));
        assert!(rendered.contains("keep {plan_id}"));
    }

    #[test]
    fn compose_orchestrator_golden() {
        let rendered = compose_role_identity(RoleIdentityContext {
            role: "Orchestrator",
            team_id: "team_alpha",
            team_name: "Alpha Team",
            plan_id: "plan-alpha",
            project: "project-alpha",
            role_guidance: "Orchestrator guidance fixture.",
        });
        assert_eq!(
            rendered,
            golden(include_str!(
                "fixtures/role_identity/orchestrator.golden.md"
            ))
        );
    }

    #[test]
    fn compose_planner_golden() {
        let rendered = compose_role_identity(RoleIdentityContext {
            role: "Planner",
            team_id: "team_beta",
            team_name: "Beta Team",
            plan_id: "plan-beta",
            project: "project-beta",
            role_guidance: "Planner guidance fixture.",
        });
        assert_eq!(
            rendered,
            golden(include_str!("fixtures/role_identity/planner.golden.md"))
        );
    }
}
