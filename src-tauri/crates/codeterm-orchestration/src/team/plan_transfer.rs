use super::*;

pub(super) fn transfer_plan_request(
    mux: &MultiplexerState,
    plan_store: &codeterm_service_core::service::plan::PlanStore,
    plan_id: &str,
    to: codeterm_service_core::plan::OrchestratorRef,
    reason: &str,
    caller: TeamMutationCaller,
) -> Result<codeterm_service_core::service::agent_transition::AgentTransitionRequest, TeamPlanError>
{
    let source_plan = plan_store
        .plan_identity_projection(plan_id)
        .map_err(TeamPlanError::Plan)?;
    let source_team_id = source_plan
        .team_id
        .as_deref()
        .filter(|team_id| !team_id.trim().is_empty())
        .ok_or_else(|| {
            TeamPlanError::Plan(PlanError::ActivePlanRequiresTeam {
                plan_id: plan_id.into(),
            })
        })?;
    let tab_id = to
        .tab_id
        .as_deref()
        .filter(|tab_id| !tab_id.trim().is_empty())
        .ok_or_else(|| {
            TeamPlanError::Team("plan transfer requires a destination Orchestrator tab".into())
        })?
        .to_string();
    let tab = mux
        .get_tab(&tab_id)
        .ok_or_else(|| TeamPlanError::Team(format!("Destination tab {tab_id} not found")))?;
    if !tab.anchors_team() {
        return Err(TeamPlanError::Team(format!(
            "Destination tab {tab_id} is not an Orchestrator"
        )));
    }
    let destination_team_id = tab
        .team_id
        .as_deref()
        .filter(|team_id| mux.get_team(team_id).is_some())
        .ok_or_else(|| {
            TeamPlanError::Team(format!(
                "Destination Orchestrator {tab_id} does not belong to a live Team"
            ))
        })?
        .to_string();
    authorize_plan_transfer(mux, source_team_id, &destination_team_id, &tab_id, caller)
        .map_err(TeamPlanError::Team)?;
    codeterm_service_core::service::agent_transition::plan_owner_request(
        to,
        plan_id,
        &destination_team_id,
        reason,
        false,
        Some((plan_id.to_string(), None)),
        true,
    )
    .map_err(TeamPlanError::Team)
}

pub(super) fn authorize_plan_transfer(
    mux: &MultiplexerState,
    source_team_id: &str,
    destination_team_id: &str,
    destination_owner_tab_id: &str,
    caller: TeamMutationCaller,
) -> Result<(), String> {
    reject_remote_team_mutation(mux, source_team_id)?;
    reject_remote_team_mutation(mux, destination_team_id)?;
    let source_authority = authorize_plan_orchestrator(mux, source_team_id, &caller);
    let source_denial = match source_authority {
        Ok(()) => return Ok(()),
        Err(denial) => denial,
    };
    match caller {
        TeamMutationCaller::AgentTab(tab_id) if tab_id == destination_owner_tab_id => {
            authorize_team_manager(
                mux,
                destination_team_id,
                &TeamMutationCaller::AgentTab(tab_id),
            )
        }
        TeamMutationCaller::AgentTab(tab_id) => Err(format!(
            "Caller tab {tab_id} lacks plan rebind authority: a source Team {source_team_id} \
Orchestrator or destination Orchestrator {destination_owner_tab_id} in Team \
{destination_team_id} must authorize the transfer. Safe recovery: old-team member \
transfers ownership, then new owner adopts."
        )),
        // A human has no destination-owner identity to fall back on, so the
        // source refusal (missing or remote-owned Team) stands verbatim.
        TeamMutationCaller::AuthenticatedMeshPeer(_)
        | TeamMutationCaller::Human
        | TeamMutationCaller::HumanConfirmedHeaderless
        | TeamMutationCaller::Unattributed => Err(source_denial),
    }
}

pub(super) fn authorize_plan_orchestrator(
    mux: &MultiplexerState,
    team_id: &str,
    caller: &TeamMutationCaller,
) -> Result<(), String> {
    authorize_team_mutation(mux, team_id, caller)?;
    if caller
        .tab_id()
        .is_some_and(|id| !mux.get_tab(id).is_some_and(|tab| tab.anchors_team()))
    {
        return Err("Plan transfer or adoption requires Orchestrator authority".into());
    }
    Ok(())
}
