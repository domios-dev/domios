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
pub(crate) struct ManagerBootstrapInputs {
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
    pub(crate) fn compose(&self) -> String {
        let role_guidance = crate::service::role_preset::manager_guidance(&self.role_kind);
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
    fn shared_builder_reminder_output_byte_identical() {
        let fixtures = [
            (
                "Orchestrator",
                "team_a",
                "Alpha",
                "pl_a",
                "api",
                "own execution",
            ),
            (
                "Planner",
                "team_b",
                "Beta",
                "pl_b",
                "desktop",
                "draft planning",
            ),
            (
                "Planner",
                "team_c",
                "Gamma",
                "pl_c",
                "cli",
                "wait for a goal",
            ),
        ];
        for (role, team_id, team_name, plan_id, project, guidance) in fixtures {
            let context = RoleIdentityContext {
                role,
                team_id,
                team_name,
                plan_id,
                project,
                role_guidance: guidance,
            };
            let before = golden(crate::prompts::MANAGER_BOOTSTRAP_TEMPLATE)
                .replace("{role}", role)
                .replace("{team_id}", team_id)
                .replace("{team_name}", team_name)
                .replace("{plan_id}", plan_id)
                .replace("{project}", project)
                .replace("{role_guidance}", guidance);
            assert_eq!(compose_role_identity(context), before);
        }
    }

    #[test]
    fn pair_bootstrap_never_fabricates_team_or_plan_ids() {
        let text = ManagerBootstrapInputs {
            role: "Planner".into(),
            team_id: "not yet established".into(),
            team_name: "not yet established".into(),
            plan_id: "no plan bound yet".into(),
            project: "CodeTerm".into(),
            coordination: None,
        }
        .compose();
        assert!(text.contains("not yet established"));
        assert!(text.contains("no plan bound yet"));
        assert!(!text.contains("team_"));
        assert!(!text.contains("pl_"));
    }

    #[test]
    fn counterpart_coordination_is_part_of_the_single_bootstrap_payload() {
        let coordination = "Your Planner counterpart is Euler (tab planner-1).";
        let mut inputs = ManagerBootstrapInputs {
            role: "Orchestrator".into(),
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
