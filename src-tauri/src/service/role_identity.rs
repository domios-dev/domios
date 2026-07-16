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
    pub role: String,
    pub team_id: String,
    pub team_name: String,
    pub plan_id: String,
    pub project: String,
}

impl ManagerBootstrapInputs {
    pub(crate) fn compose(&self) -> String {
        let role_guidance = if self.role == "Orchestrator" {
            crate::prompts::orchestrator_suffix()
        } else {
            crate::prompts::planner_suffix()
        };
        compose_role_identity(RoleIdentityContext {
            role: &self.role,
            team_id: &self.team_id,
            team_name: &self.team_name,
            plan_id: &self.plan_id,
            project: &self.project,
            role_guidance: &role_guidance,
        })
    }
}

pub(crate) fn compose_role_identity(context: RoleIdentityContext<'_>) -> String {
    crate::prompts::MANAGER_BOOTSTRAP_TEMPLATE
        .replace("{role}", context.role)
        .replace("{team_id}", context.team_id)
        .replace("{team_name}", context.team_name)
        .replace("{plan_id}", context.plan_id)
        .replace("{project}", context.project)
        .replace("{role_guidance}", context.role_guidance)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            let before = crate::prompts::MANAGER_BOOTSTRAP_TEMPLATE
                .replace("{role}", role)
                .replace("{team_id}", team_id)
                .replace("{team_name}", team_name)
                .replace("{plan_id}", plan_id)
                .replace("{project}", project)
                .replace("{role_guidance}", guidance);
            assert_eq!(compose_role_identity(context), before);
        }
    }
}
