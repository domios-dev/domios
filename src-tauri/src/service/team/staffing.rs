use codeterm_types::agent::role_kind::{self, builtins::ORCHESTRATOR, builtins::PLANNER};
use codeterm_types::team::general_agent_membership::MembershipAttempt;
use codeterm_types::team::staffing as codes;
use codeterm_types::team::Team;

use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{AgentRole, TabCloseReason};
use crate::service::general_agent::{refuse_general_agent_team_membership, shape_for_tab};
use crate::service::orchestrator_onboard::tab_is_agent_capable;
use crate::service::team::{authorize_team_manager, TeamMutationCaller};
use crate::service::ServiceContext;

/// Every refusal leaves the same trace: the Team, the role that was asked for,
/// the code, and the cause behind it.
fn refuse(team_id: &str, role: &str, code: &'static str, cause: impl Into<String>) -> String {
    let cause = cause.into();
    log::warn!("[team-staffing] add refused team={team_id} role={role} code={code} cause={cause}");
    format!("{code}: {cause}")
}

#[derive(Debug, Clone)]
pub(crate) struct AddTeamMemberParams {
    pub provider: String,
    pub role: String,
    pub title: Option<String>,
    pub task: Option<String>,
    pub model: Option<String>,
}

/// Where the Team's plan landed when an added anchor took it over.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct PlanTakeover {
    pub plan_id: String,
    pub owner_tab_id: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct AddTeamMemberResult {
    pub tab_id: String,
    pub team: Team,
    pub plan_takeover: Option<PlanTakeover>,
}

#[derive(Debug)]
pub(crate) struct PreparedTeamMemberSpawn {
    pub spawn_params: crate::service::SpawnAgentParams,
    pub expected_role: String,
    pub expected_profile: Option<String>,
}

/// Orchestrators that still hold this Team. A removed tab is gone from the mux
/// and a dead agent's identity is cleared on lifecycle exit, so agent capability
/// is what separates a live anchor from one that is only a record.
pub(crate) fn live_team_orchestrators(mux: &MultiplexerState, team_id: &str) -> Vec<String> {
    let mut anchors: Vec<String> = mux
        .snapshot()
        .tabs
        .values()
        .filter(|tab| {
            tab.team_id.as_deref() == Some(team_id)
                && tab.anchors_team()
                && tab_is_agent_capable(tab)
        })
        .map(|tab| tab.id.clone())
        .collect();
    anchors.sort();
    anchors
}

pub(crate) fn prepare_team_member_spawn(
    mux: &MultiplexerState,
    team_id: &str,
    caller_tab_id: Option<&str>,
    params: AddTeamMemberParams,
) -> Result<PreparedTeamMemberSpawn, String> {
    let requested = params.role.clone();
    if let Some(caller_tab_id) = caller_tab_id {
        // Guarded before authorization so the General Agent gets the teaching refusal whether or not it is already (wrongly) in the Team.
        let shape = shape_for_tab(mux, caller_tab_id).with_agents(Some(&params.provider), None);
        refuse_general_agent_team_membership(
            mux,
            caller_tab_id,
            MembershipAttempt::ExerciseMembership,
            &shape,
        )?;
    }
    authorize_team_manager(mux, team_id, &TeamMutationCaller::from_tab(caller_tab_id))
        .map_err(|cause| refuse(team_id, &requested, codes::TEAM_CALLER_UNAUTHORIZED, cause))?;
    if let Some(caller_tab_id) = caller_tab_id {
        if !mux
            .get_tab(caller_tab_id)
            .is_some_and(|caller| caller.can_spawn)
        {
            return Err(refuse(
                team_id,
                &requested,
                codes::TEAM_CALLER_CANNOT_SPAWN,
                format!("Caller tab {caller_tab_id} does not allow spawning Team members"),
            ));
        }
    }
    let (role, role_profile) = team_member_role(&params.role)
        .map_err(|cause| refuse(team_id, &requested, codes::TEAM_MEMBER_ROLE_UNKNOWN, cause))?;
    let anchors =
        role_kind::anchors_team(AgentRole::from_str_compat(&role), role_profile.as_deref());
    // The Team-shaped invariant is a headcount of live anchors, not a ban on the
    // role: a Team that lost its Orchestrator must be able to receive another.
    if anchors {
        let live = live_team_orchestrators(mux, team_id);
        if !live.is_empty() {
            return Err(refuse(
                team_id,
                &requested,
                codes::TEAM_ORCHESTRATOR_ALREADY_LIVE,
                format!(
                    "Team {team_id} already has a live Orchestrator ({}); remove it before adding another",
                    live.join(", ")
                ),
            ));
        }
    }
    // A caller-less add is the bootstrap door: a Team whose members cannot spawn would otherwise have no way to receive the manager they are told to route through.
    if caller_tab_id.is_none() && !anchors {
        return Err(refuse(
            team_id,
            &requested,
            codes::TEAM_ROLE_REQUIRES_SPAWNER,
            format!("{role} takes a parent, so it needs a Team member that can spawn"),
        ));
    }
    let workspace_id = match caller_tab_id {
        Some(caller_tab_id) => mux.find_workspace_for_tab(caller_tab_id).ok_or_else(|| {
            refuse(
                team_id,
                &requested,
                codes::TEAM_WORKSPACE_UNRESOLVED,
                format!("Caller tab {caller_tab_id} has no workspace"),
            )
        })?,
        None => mux
            .get_team(team_id)
            .map(|team| team.workspace_id.clone())
            .ok_or_else(|| {
                refuse(
                    team_id,
                    &requested,
                    codes::TEAM_WORKSPACE_UNRESOLVED,
                    format!("Team {team_id} not found"),
                )
            })?,
    };
    let parent_tab_id = (!anchors).then(|| caller_tab_id.unwrap_or_default().into());
    let task = params.task.filter(|task| !task.trim().is_empty());
    let starter_prompt = crate::service::spawn_params::starter_prompt_for_creation(
        Some(role.as_str()),
        task.as_deref(),
    );
    Ok(PreparedTeamMemberSpawn {
        expected_role: role.clone(),
        expected_profile: role_profile.clone(),
        spawn_params: crate::service::SpawnAgentParams {
            agent: Some(params.provider),
            title: params.title,
            role: Some(role),
            role_profile,
            parent_tab_id,
            task,
            starter_prompt,
            model: params.model,
            workspace_id: Some(workspace_id),
            report_target_tab_id: caller_tab_id.map(Into::into),
            spawned_by_tab_id: caller_tab_id.map(Into::into),
            ..Default::default()
        },
    })
}

pub(crate) fn prepare_team_member_spawn_with_provider_validation<F>(
    mux: &MultiplexerState,
    team_id: &str,
    caller_tab_id: Option<&str>,
    params: AddTeamMemberParams,
    validate_provider: F,
) -> Result<PreparedTeamMemberSpawn, String>
where
    F: FnOnce(&crate::service::SpawnAgentParams) -> Result<(), String>,
{
    let prepared = prepare_team_member_spawn(mux, team_id, caller_tab_id, params)?;
    validate_provider(&prepared.spawn_params)?;
    Ok(prepared)
}

pub(crate) fn commit_team_member_spawn(
    mux: &mut MultiplexerState,
    events: &crate::api::events::EventBroadcaster,
    team_id: &str,
    caller_tab_id: Option<&str>,
    tab_id: &str,
    expected_role: &str,
    expected_profile: &Option<String>,
) -> Result<AddTeamMemberResult, String> {
    let (expected_role, expected_kind) =
        ServiceContext::parse_role_kind(expected_role, expected_profile.as_deref())?;
    // An anchoring member takes no parent and binds its Team after placement.
    let anchors = role_kind::anchors_team(Some(expected_role), expected_kind.as_deref());
    // Resolve the expectation through the same rule the tab reports itself by:
    // a profileless Worker is the `worker` kind on both sides, never `None`.
    let expected_kind_id = role_kind::kind_id(Some(expected_role), expected_kind.as_deref());
    let expected_parent = (!anchors).then_some(caller_tab_id).flatten();
    let tab = mux
        .get_tab(tab_id)
        .ok_or_else(|| format!("Tab {tab_id} not found after member spawn"))?;
    // With no caller there is no caller workspace to compare against, so the Team's
    // own workspace is the expectation — the same one the spawn was given.
    let expected_workspace = match caller_tab_id {
        Some(caller_tab_id) => mux.find_workspace_for_tab(caller_tab_id),
        None => mux.get_team(team_id).map(|team| team.workspace_id.clone()),
    };
    let invalid_hierarchy = tab.parent_tab_id.as_deref() != expected_parent
        || (!anchors && tab.team_id.as_deref() != Some(team_id))
        || tab.role != Some(expected_role)
        || tab.role_kind_id() != expected_kind_id
        || mux.find_workspace_for_tab(tab_id) != expected_workspace;
    if invalid_hierarchy {
        mux.close_tab_with_reason(tab_id, TabCloseReason::SpawnRollback)?;
        return Err("Team member spawn did not preserve the manager hierarchy".into());
    }
    if anchors {
        let shape = shape_for_tab(mux, tab_id);
        refuse_general_agent_team_membership(mux, tab_id, MembershipAttempt::Join, &shape)?;
        mux.get_tab_mut(tab_id)
            .expect("Tab validated before Team assignment")
            .team_id = Some(team_id.into());
    }
    let team = mux
        .get_team(team_id)
        .cloned()
        .ok_or_else(|| format!("Team {team_id} not found after member spawn"))?;
    crate::service::publish_mux_state_changed(events);
    Ok(AddTeamMemberResult {
        tab_id: tab_id.into(),
        team,
        plan_takeover: None,
    })
}

/// Hand the Team's active plan to an anchor that just joined. A plan whose
/// recorded owner is still a live anchor here is left alone; everything else is
/// the tab-gone case the recovery rebind exists for.
pub(crate) fn take_over_team_plan(
    mux: &mut MultiplexerState,
    plan_store: &crate::service::plan::PlanStore,
    team_id: &str,
    owner_tab_id: &str,
    now_ms: u64,
) -> Option<PlanTakeover> {
    let (plan_id, _) = plan_store.active_plan_for_team(team_id)?;
    let previous = plan_store
        .get(&plan_id)
        .and_then(|plan| plan.owner.tab_id)
        .unwrap_or_default();
    if previous == owner_tab_id {
        return None;
    }
    let previous_is_live_anchor = mux.get_tab(&previous).is_some_and(|tab| {
        tab.anchors_team() && tab.team_id.as_deref() == Some(team_id) && tab_is_agent_capable(tab)
    });
    if previous_is_live_anchor {
        return None;
    }
    let reason = match previous.is_empty() {
        true => format!("Orchestrator recovery: Team {team_id} plan had no owner tab"),
        false => format!("Orchestrator recovery: owner tab {previous} is gone"),
    };
    let error = crate::service::agent_transition::recover_plan_ownership_for_new_orchestrator(
        mux,
        plan_store,
        &plan_id,
        crate::plan::OrchestratorRef::from_tab(owner_tab_id),
        team_id,
        &reason,
        now_ms,
    )
    .err();
    match &error {
        Some(cause) => log::error!(
            "[team-staffing] plan takeover failed team={team_id} plan={plan_id} owner={owner_tab_id} cause={cause}"
        ),
        None => log::info!(
            "[team-staffing] plan takeover team={team_id} plan={plan_id} owner={owner_tab_id}"
        ),
    }
    Some(PlanTakeover {
        plan_id,
        owner_tab_id: owner_tab_id.to_string(),
        error,
    })
}

/// The anchoring question answered from an already-resolved role pair.
pub(crate) fn role_anchors_team(role: &str, role_profile: Option<&str>) -> bool {
    role_kind::anchors_team(AgentRole::from_str_compat(role), role_profile)
}

pub(crate) fn team_member_role(requested: &str) -> Result<(String, Option<String>), String> {
    match requested.trim().to_ascii_lowercase().as_str() {
        "worker" => Ok(("Worker".into(), None)),
        "planner" => Ok(("Manager".into(), Some(PLANNER.into()))),
        "orchestrator" => Ok(("Manager".into(), Some(ORCHESTRATOR.into()))),
        other => Err(format!(
            "Team member role must be worker, planner, or orchestrator, got '{other}'"
        )),
    }
}

#[cfg(test)]
mod tests;
