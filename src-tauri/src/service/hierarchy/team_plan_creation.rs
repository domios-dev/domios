use codeterm_types::team::general_agent_membership::MembershipAttempt;

use crate::multiplexer::commands::MuxState;
use crate::multiplexer::types::AgentRole;
use crate::plan::{MilestoneKind, OrchestratorRef, Plan};
use crate::service::plan::PlanError;
use crate::service::plan::PlanStore;
use crate::service::ServiceContext;

#[derive(Debug)]
pub(crate) enum TeamPlanCreationError {
    Team(String),
    Plan(PlanError),
}

#[derive(Debug, Clone)]
pub(crate) struct TeamPlanCreationOutcome {
    pub plan: Plan,
    pub team_id: String,
    pub team_minted: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TeamPlanCaller {
    pub tab_id: Option<String>,
    pub session_id: Option<String>,
    pub binding_nonce: Option<String>,
}

impl TeamPlanCreationOutcome {
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
    pub(crate) fn create_team_plan_for_orchestrator(
        &self,
        title: String,
        project_path: String,
        owner: OrchestratorRef,
        kinds: Vec<MilestoneKind>,
        from_spec: Option<String>,
        now_ms: u64,
    ) -> Result<TeamPlanCreationOutcome, TeamPlanCreationError> {
        let outcome = create_team_plan_for_orchestrator(
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

    pub(crate) fn create_team_plan_for_caller(
        &self,
        title: String,
        project_path: String,
        requested_owner: OrchestratorRef,
        caller: TeamPlanCaller,
        kinds: Vec<MilestoneKind>,
        from_spec: Option<String>,
        now_ms: u64,
    ) -> Result<TeamPlanCreationOutcome, TeamPlanCreationError> {
        let outcome = create_team_plan_for_caller(
            &self.mux,
            &self.plan_store,
            title,
            project_path,
            requested_owner,
            caller,
            kinds,
            from_spec,
            now_ms,
        )?;
        self.emit_state_changed();
        Ok(outcome)
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn create_team_plan_for_caller(
    mux: &MuxState,
    plan_store: &PlanStore,
    title: String,
    project_path: String,
    requested_owner: OrchestratorRef,
    caller: TeamPlanCaller,
    kinds: Vec<MilestoneKind>,
    from_spec: Option<String>,
    now_ms: u64,
) -> Result<TeamPlanCreationOutcome, TeamPlanCreationError> {
    let Some(caller_tab_id) = caller.tab_id.as_deref().filter(|value| !value.is_empty()) else {
        return create_team_plan_for_orchestrator(
            mux,
            plan_store,
            title,
            project_path,
            requested_owner,
            kinds,
            from_spec,
            now_ms,
        );
    };
    let mut state = mux.write();
    let caller_tab = state
        .get_tab(caller_tab_id)
        .ok_or_else(|| TeamPlanCreationError::Team("the claimed Planner tab is not live".into()))?;
    if caller_tab.anchors_team() {
        drop(state);
        return create_team_plan_for_orchestrator(
            mux,
            plan_store,
            title,
            project_path,
            requested_owner,
            kinds,
            from_spec,
            now_ms,
        );
    }
    let requested_session_id = caller
        .session_id
        .as_deref()
        .filter(|value| !value.is_empty());
    let session_id = requested_session_id.or(caller_tab.agent_session_id.as_deref());
    let binding_nonce = caller
        .binding_nonce
        .as_deref()
        .filter(|value| !value.is_empty());
    let live_identity = requested_session_id.is_none_or(|requested| Some(requested) == session_id)
        && caller_tab.agent_session_id.as_deref() == session_id
        && caller_tab.session_binding_nonce.as_deref() == binding_nonce
        && caller_tab.reported_session_binding_nonce.as_deref() == binding_nonce
        && session_id.is_some()
        && binding_nonce.is_some();
    let owner_tab_id = caller_tab.parent_tab_id.clone().ok_or_else(|| {
        TeamPlanCreationError::Team("the caller is not a structurally assigned Team Planner".into())
    })?;
    let team_id = caller_tab.team_id.clone().ok_or_else(|| {
        TeamPlanCreationError::Team("the caller is not assigned to a Team".into())
    })?;
    if !live_identity {
        return Err(TeamPlanCreationError::Team(
            "the live Team Planner session is required".into(),
        ));
    }
    if caller_tab.role != Some(AgentRole::Manager)
        || caller_tab.role_profile.as_deref() != Some("planner")
    {
        return Err(TeamPlanCreationError::Team(
            "the caller is not a Planner in this Team".into(),
        ));
    }
    let owner_tab = state
        .get_tab(&owner_tab_id)
        .ok_or_else(|| TeamPlanCreationError::Team("the Team Orchestrator is not live".into()))?;
    if !owner_tab.anchors_team() || owner_tab.team_id.as_deref() != Some(team_id.as_str()) {
        return Err(TeamPlanCreationError::Team(
            "the caller is not bound to a live Team Orchestrator".into(),
        ));
    }
    let owner = OrchestratorRef {
        tab_id: Some(owner_tab_id.clone()),
        session_id: owner_tab.agent_session_id.clone(),
        title: Some(owner_tab.title.clone()),
    };
    let create = if let Some(spec_id) = from_spec {
        plan_store.create_plan_from_spec_for_team(
            &spec_id,
            title,
            owner,
            team_id.clone(),
            kinds,
            now_ms,
        )
    } else {
        plan_store.create_plan_for_team(title, project_path, owner, team_id.clone(), kinds, now_ms)
    };
    let plan = create.map_err(TeamPlanCreationError::Plan)?;
    if let Some(owner_tab) = state.get_tab_mut(&owner_tab_id) {
        if owner_tab.pending_paired_planner_tab_id.as_deref() == Some(caller_tab_id) {
            owner_tab.pending_paired_planner_tab_id = None;
        }
    }
    Ok(TeamPlanCreationOutcome {
        plan,
        team_id,
        team_minted: false,
    })
}

pub(crate) fn create_team_plan_for_orchestrator(
    mux: &MuxState,
    plan_store: &PlanStore,
    title: String,
    project_path: String,
    owner: OrchestratorRef,
    kinds: Vec<MilestoneKind>,
    from_spec: Option<String>,
    now_ms: u64,
) -> Result<TeamPlanCreationOutcome, TeamPlanCreationError> {
    let owner_tab_id = owner
        .tab_id
        .as_deref()
        .filter(|tab_id| !tab_id.is_empty())
        .ok_or_else(|| {
            TeamPlanCreationError::Team("Team Plan creation requires an owner Tab".into())
        })?
        .to_string();
    let mut state = mux.write();
    let owner_anchors = state
        .get_tab(&owner_tab_id)
        .ok_or_else(|| TeamPlanCreationError::Team(format!("Tab {owner_tab_id} not found")))?
        .anchors_team();
    if !owner_anchors {
        return Err(TeamPlanCreationError::Team(format!(
            "Tab {owner_tab_id} is not an Orchestrator"
        )));
    }
    // Membership door 4: minting a plan's Team writes it onto the owner Tab,
    // and the General Agent kind anchors, so it would qualify without this.
    let shape = crate::service::general_agent::shape_for_tab(&state, &owner_tab_id);
    crate::service::general_agent::refuse_general_agent_team_membership(
        &state,
        &owner_tab_id,
        MembershipAttempt::Join,
        &shape,
    )
    .map_err(|refusal| TeamPlanCreationError::Team(refusal.to_string()))?;
    let team = state
        .resolve_or_mint_team_for_parent(&owner_tab_id)
        .map_err(TeamPlanCreationError::Team)?;
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
        plan_store.create_plan_for_team(
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
                state.rollback_minted_team_for_parent(&owner_tab_id, &team.team_id);
            }
            return Err(TeamPlanCreationError::Plan(error));
        }
    };
    Ok(TeamPlanCreationOutcome {
        plan,
        team_id: team.team_id,
        team_minted: team.minted,
    })
}

#[cfg(test)]
mod tests;
