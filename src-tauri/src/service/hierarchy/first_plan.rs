use crate::multiplexer::types::AgentRole;
use crate::plan::{MilestoneKind, OrchestratorRef, Plan};
use crate::service::ServiceContext;

#[derive(Debug, Clone)]
pub(crate) struct FirstPlanTeamOutcome {
    pub plan: Plan,
    pub team_id: String,
    pub team_minted: bool,
}

impl ServiceContext {
    pub(crate) fn create_first_plan_for_orchestrator(
        &self,
        title: String,
        project_path: String,
        owner: OrchestratorRef,
        kinds: Vec<MilestoneKind>,
        now_ms: u64,
    ) -> Result<FirstPlanTeamOutcome, String> {
        let owner_pane_id = owner
            .pane_id
            .as_deref()
            .filter(|pane_id| !pane_id.is_empty())
            .ok_or("first-plan creation requires an owner pane")?;
        let mut mux = self.mux.write();
        let owner_role = mux
            .get_pane(owner_pane_id)
            .ok_or_else(|| format!("Pane {owner_pane_id} not found"))?
            .role;
        if owner_role != Some(AgentRole::Orchestrator) {
            return Err(format!("Pane {owner_pane_id} is not an Orchestrator"));
        }
        let team = mux.resolve_or_mint_team_for_parent(owner_pane_id)?;
        let plan = match self.plan_store.create_plan_for_team(
            title,
            project_path,
            owner,
            Some(team.team_id.clone()),
            kinds,
            now_ms,
        ) {
            Ok(plan) => plan,
            Err(error) => {
                if team.minted {
                    mux.rollback_minted_team_for_parent(owner_pane_id, &team.team_id);
                }
                return Err(error.to_string());
            }
        };
        drop(mux);
        self.emit_state_changed();
        Ok(FirstPlanTeamOutcome {
            plan,
            team_id: team.team_id,
            team_minted: team.minted,
        })
    }
}
