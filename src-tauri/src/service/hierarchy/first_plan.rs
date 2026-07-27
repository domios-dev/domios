use crate::multiplexer::commands::MuxState;
use crate::multiplexer::types::AgentRole;
use crate::plan::{MilestoneKind, OrchestratorRef, Plan};
use crate::service::plan::PlanError;
use crate::service::plan::PlanStore;
use crate::service::ServiceContext;

#[derive(Debug)]
pub(crate) enum FirstPlanTeamError {
    Team(String),
    Plan(PlanError),
}

#[derive(Debug, Clone)]
pub(crate) struct FirstPlanTeamOutcome {
    pub plan: Plan,
    pub team_id: String,
    pub team_minted: bool,
}

impl FirstPlanTeamOutcome {
    pub(crate) fn apply_to_response(&self, response: &mut serde_json::Value) {
        response["team_id"] = serde_json::json!(self.team_id);
        response["team_minted"] = serde_json::json!(self.team_minted);
        if !self.team_minted {
            return;
        }
        let reminder = format!(
            "Name this Team: codeterm team rename {} <name>; then read codeterm docs ORCHESTRATION.",
            self.team_id
        );
        if let Some(hints) = response
            .get_mut("_hints")
            .and_then(|value| value.as_array_mut())
        {
            hints.push(serde_json::json!(reminder));
        } else {
            response["_hints"] = serde_json::json!([reminder]);
        }
    }
}

impl ServiceContext {
    pub(crate) fn create_first_plan_for_orchestrator(
        &self,
        title: String,
        project_path: String,
        owner: OrchestratorRef,
        kinds: Vec<MilestoneKind>,
        from_spec: Option<String>,
        now_ms: u64,
    ) -> Result<FirstPlanTeamOutcome, FirstPlanTeamError> {
        let outcome = create_first_plan_for_orchestrator(
            &self.mux,
            &self.plan_store,
            title,
            project_path,
            owner,
            kinds,
            from_spec,
            now_ms,
        )?;
        self.emit_state_changed();
        Ok(outcome)
    }
}

pub(crate) fn create_first_plan_for_orchestrator(
    mux: &MuxState,
    plan_store: &PlanStore,
    title: String,
    project_path: String,
    owner: OrchestratorRef,
    kinds: Vec<MilestoneKind>,
    from_spec: Option<String>,
    now_ms: u64,
) -> Result<FirstPlanTeamOutcome, FirstPlanTeamError> {
    let owner_pane_id = owner
        .pane_id
        .as_deref()
        .filter(|pane_id| !pane_id.is_empty())
        .ok_or_else(|| {
            FirstPlanTeamError::Team("first-plan creation requires an owner Tab".into())
        })?
        .to_string();
    let mut state = mux.write();
    let owner_role = state
        .get_pane(&owner_pane_id)
        .ok_or_else(|| FirstPlanTeamError::Team(format!("Tab {owner_pane_id} not found")))?
        .role;
    if owner_role != Some(AgentRole::Orchestrator) {
        return Err(FirstPlanTeamError::Team(format!(
            "Tab {owner_pane_id} is not an Orchestrator"
        )));
    }
    let team = state
        .resolve_or_mint_team_for_parent(&owner_pane_id)
        .map_err(FirstPlanTeamError::Team)?;
    let create = if let Some(spec_id) = from_spec {
        plan_store.create_plan_from_spec_for_team(
            &spec_id,
            title,
            owner,
            team.team_id.clone(),
            kinds,
            now_ms,
        )
    } else {
        plan_store.create_first_plan_for_team(
            title,
            project_path,
            owner,
            team.team_id.clone(),
            kinds,
            now_ms,
        )
    };
    let plan = match create {
        Ok(plan) => plan,
        Err(error) => {
            if team.minted {
                state.rollback_minted_team_for_parent(&owner_pane_id, &team.team_id);
            }
            return Err(FirstPlanTeamError::Plan(error));
        }
    };
    Ok(FirstPlanTeamOutcome {
        plan,
        team_id: team.team_id,
        team_minted: team.minted,
    })
}

#[cfg(test)]
mod tests;
