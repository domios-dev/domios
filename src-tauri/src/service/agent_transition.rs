use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{AgentRole, Tab};
use crate::plan::PlannerRef;
use crate::service::plan::mutations::agent_lifecycle::{
    AgentLifecyclePlanChange, AgentLifecyclePlanRebind, AgentLifecyclePlanTransaction,
};
use crate::service::ServiceContext;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentPlacement {
    Roleless,
    Worker {
        parent_tab_id: String,
    },
    Planner {
        plan_id: String,
    },
    Manager {
        parent_tab_id: String,
        role_profile: Option<String>,
    },
    Orchestrator {
        can_spawn: bool,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct AgentTransitionRequest {
    pub tab_id: String,
    pub destination: AgentPlacement,
    pub require_verified_session: bool,
    pub planner_replacement: Option<(String, Option<PlannerRef>)>,
    pub plan_rebind: Option<AgentLifecyclePlanRebind>,
}

impl AgentTransitionRequest {
    pub(crate) fn roleless(tab_id: impl Into<String>) -> Self {
        Self {
            tab_id: tab_id.into(),
            destination: AgentPlacement::Roleless,
            require_verified_session: false,
            planner_replacement: None,
            plan_rebind: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransitionFailurePoint {
    None,
    AfterPlanCommit,
    SkipPlanRebind,
}

struct PreparedTransition {
    staged_mux: crate::multiplexer::state::TeamCloseTransaction,
    plan_changes: Vec<AgentLifecyclePlanChange>,
    expected_rebind: Option<AgentLifecyclePlanRebind>,
}

impl ServiceContext {
    pub fn set_tab_role(
        &self,
        tab_id: &str,
        role_str: &str,
        parent_tab_id: Option<&str>,
        can_spawn: Option<bool>,
    ) -> Result<(), String> {
        self.set_tab_role_with_profile(tab_id, role_str, None, parent_tab_id, can_spawn)
    }

    pub(crate) fn set_tab_role_with_profile(
        &self,
        tab_id: &str,
        role_str: &str,
        role_profile: Option<&str>,
        parent_tab_id: Option<&str>,
        can_spawn: Option<bool>,
    ) -> Result<(), String> {
        let destination = match Self::parse_role(role_str)? {
            AgentRole::Orchestrator => AgentPlacement::Orchestrator {
                can_spawn: can_spawn.unwrap_or(true),
            },
            AgentRole::Manager => AgentPlacement::Manager {
                parent_tab_id: parent_tab_id
                    .ok_or("Manager requires a parent_tab_id")?
                    .to_string(),
                role_profile: role_profile.map(str::to_string),
            },
            AgentRole::Worker => AgentPlacement::Worker {
                parent_tab_id: parent_tab_id
                    .ok_or("Worker requires a parent_tab_id")?
                    .to_string(),
            },
        };
        self.transition_agent(AgentTransitionRequest {
            tab_id: tab_id.to_string(),
            destination,
            require_verified_session: false,
            planner_replacement: None,
            plan_rebind: None,
        })?;
        Ok(())
    }

    pub fn clear_tab_role(&self, tab_id: &str) -> Result<(), String> {
        match self.transition_agent(AgentTransitionRequest::roleless(tab_id)) {
            Ok(_) => Ok(()),
            Err(error) if error.contains("not found") => {
                let touched = self.remote_workers.forget_by_remote_tab(tab_id, None, None);
                if touched.is_empty() {
                    return Err(error);
                }
                for orchestrator in touched {
                    self.emit_remote_workers_changed(&orchestrator);
                }
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    pub(crate) fn transition_agent(
        &self,
        request: AgentTransitionRequest,
    ) -> Result<Vec<String>, String> {
        self.transition_agent_with_failure(request, TransitionFailurePoint::None)
    }

    fn transition_agent_with_failure(
        &self,
        request: AgentTransitionRequest,
        failure: TransitionFailurePoint,
    ) -> Result<Vec<String>, String> {
        let mut mux = self.mux.write();
        let changed_plan_ids = execute_transition(&mut mux, &self.plan_store, request, failure)?;
        drop(mux);
        self.emit_state_changed();
        for plan_id in &changed_plan_ids {
            self.emit_plan_changed(plan_id);
        }
        Ok(changed_plan_ids)
    }

    pub(crate) fn transition_plan_planner(
        &self,
        plan_id: &str,
        planner: Option<PlannerRef>,
    ) -> Result<Vec<String>, String> {
        if let Some(PlannerRef::Tab { tab_id, .. }) = &planner {
            return self.transition_agent(AgentTransitionRequest {
                tab_id: tab_id.clone(),
                destination: AgentPlacement::Planner {
                    plan_id: plan_id.to_string(),
                },
                require_verified_session: true,
                planner_replacement: None,
                plan_rebind: None,
            });
        }

        let current = self
            .plan_store
            .get(plan_id)
            .ok_or_else(|| format!("Plan {plan_id} not found"))?
            .planner;
        let old_tab_id = match current {
            Some(PlannerRef::Tab { tab_id, .. }) => Some(tab_id),
            _ => None,
        };
        if let Some(tab_id) = old_tab_id.as_ref() {
            if self.mux.read().get_tab(&tab_id).is_some() {
                return self.transition_agent(AgentTransitionRequest {
                    tab_id: tab_id.clone(),
                    destination: AgentPlacement::Roleless,
                    require_verified_session: false,
                    planner_replacement: Some((plan_id.to_string(), planner)),
                    plan_rebind: None,
                });
            }
        }

        let mux = self.mux.write();
        let change = AgentLifecyclePlanChange {
            tab_id: old_tab_id.unwrap_or_default(),
            preserve_owned_plans: true,
            preserve_task_bindings_for_owner: None,
            planner_binding: Some((plan_id.to_string(), planner)),
            plan_rebind: None,
        };
        let transaction = self
            .plan_store
            .apply_agent_lifecycle_change(change, epoch_ms())
            .map_err(|error| error.to_string())?;
        let changed = transaction.changed_plan_ids().to_vec();
        drop(mux);
        for changed_plan_id in &changed {
            self.emit_plan_changed(changed_plan_id);
        }
        Ok(changed)
    }
}

pub(crate) fn transition_plan_to_orchestrator_core(
    mux: &mut MultiplexerState,
    plan_store: &crate::service::plan::PlanStore,
    plan_id: &str,
    owner: crate::plan::OrchestratorRef,
    team_id: &str,
    reason: &str,
    activate: bool,
    now_ms: u64,
) -> Result<crate::plan::Plan, String> {
    let tab_id = owner
        .tab_id
        .as_deref()
        .filter(|tab_id| !tab_id.trim().is_empty())
        .ok_or_else(|| "plan rebind requires a destination Orchestrator tab".to_string())?
        .to_string();
    let rebind = AgentLifecyclePlanRebind {
        plan_id: plan_id.to_string(),
        owner,
        team_id: team_id.to_string(),
        reason: reason.to_string(),
        activate,
        clear_planner: true,
    };
    execute_transition_at(
        mux,
        plan_store,
        AgentTransitionRequest {
            tab_id,
            destination: AgentPlacement::Orchestrator { can_spawn: true },
            require_verified_session: false,
            planner_replacement: None,
            plan_rebind: Some(rebind.clone()),
        },
        TransitionFailurePoint::None,
        now_ms,
    )?;
    verify_persisted_plan_rebind(plan_store, &rebind)
}

fn execute_transition(
    mux: &mut MultiplexerState,
    plan_store: &crate::service::plan::PlanStore,
    request: AgentTransitionRequest,
    failure: TransitionFailurePoint,
) -> Result<Vec<String>, String> {
    execute_transition_at(mux, plan_store, request, failure, epoch_ms())
}

fn execute_transition_at(
    mux: &mut MultiplexerState,
    plan_store: &crate::service::plan::PlanStore,
    request: AgentTransitionRequest,
    failure: TransitionFailurePoint,
    now_ms: u64,
) -> Result<Vec<String>, String> {
    let mut prepared = prepare_transition(mux, plan_store, &request)?;
    if failure == TransitionFailurePoint::SkipPlanRebind {
        for change in &mut prepared.plan_changes {
            change.plan_rebind = None;
        }
    }
    let plan_transaction = plan_store
        .apply_agent_lifecycle_changes(prepared.plan_changes, now_ms)
        .map_err(|error| error.to_string())?;
    if failure == TransitionFailurePoint::AfterPlanCommit {
        return compensate_injected_failure(plan_store, plan_transaction);
    }
    if let Some(rebind) = &prepared.expected_rebind {
        if let Err(error) = verify_persisted_plan_rebind(plan_store, rebind) {
            return compensate_failed_postcondition(plan_store, plan_transaction, error);
        }
    }
    let changed_plan_ids = plan_transaction.changed_plan_ids().to_vec();
    prepared.staged_mux.commit(mux, &[]);
    Ok(changed_plan_ids)
}

fn prepare_transition(
    mux: &MultiplexerState,
    plan_store: &crate::service::plan::PlanStore,
    request: &AgentTransitionRequest,
) -> Result<PreparedTransition, String> {
    let source = mux
        .get_tab(&request.tab_id)
        .ok_or_else(|| format!("Tab {} not found", request.tab_id))?
        .clone();
    if request.require_verified_session {
        verified_session(&source)?;
    }

    let mut staged_mux = mux.begin_team_close_transaction();
    let staged = staged_mux.state_mut();
    let mut plan_changes = Vec::new();
    let plan_rebind = destination_plan_rebind(mux, plan_store, request, &source)?;
    let preserve_owned_plans = matches!(request.destination, AgentPlacement::Orchestrator { .. })
        && source.role == Some(AgentRole::Orchestrator)
        && source.parent_tab_id.is_none();
    let preserve_task_bindings_for_owner = match &request.destination {
        AgentPlacement::Worker { parent_tab_id }
            if source.role == Some(AgentRole::Worker)
                && source.parent_tab_id.as_deref() == Some(parent_tab_id.as_str()) =>
        {
            Some(parent_tab_id.clone())
        }
        _ => None,
    };

    let incumbent = planner_incumbent(
        plan_store,
        &request.destination,
        plan_rebind.as_ref(),
        &request.tab_id,
    );
    if let Some(incumbent_id) = incumbent.as_deref() {
        for affected_id in clear_lifecycle(staged, incumbent_id)? {
            plan_changes.push(revocation_change(affected_id));
        }
    }

    let affected = apply_destination(staged, request, preserve_owned_plans, plan_rebind.as_ref())?;
    for affected_id in affected
        .into_iter()
        .filter(|tab_id| tab_id != &request.tab_id)
    {
        plan_changes.push(revocation_change(affected_id));
    }
    relink_plan_members(staged, plan_store, plan_rebind.as_ref())?;
    let planner_binding = planner_binding(staged, request, plan_rebind.as_ref())?;
    plan_changes.push(AgentLifecyclePlanChange {
        tab_id: request.tab_id.clone(),
        preserve_owned_plans,
        preserve_task_bindings_for_owner,
        planner_binding: request.planner_replacement.clone().or(planner_binding),
        plan_rebind: plan_rebind.clone(),
    });
    verify_destination(staged, request, plan_rebind.as_ref())?;

    Ok(PreparedTransition {
        staged_mux,
        plan_changes,
        expected_rebind: plan_rebind,
    })
}

fn apply_destination(
    mux: &mut MultiplexerState,
    request: &AgentTransitionRequest,
    preserve_owned_plans: bool,
    plan_rebind: Option<&AgentLifecyclePlanRebind>,
) -> Result<Vec<String>, String> {
    if preserve_owned_plans {
        normalize_orchestrator(mux, &request.tab_id)?;
        return Ok(Vec::new());
    }

    let affected = clear_lifecycle(mux, &request.tab_id)?;
    match &request.destination {
        AgentPlacement::Roleless => {}
        AgentPlacement::Worker { parent_tab_id } => {
            mux.set_tab_role(&request.tab_id, AgentRole::Worker, Some(parent_tab_id))?;
            set_report_owner(mux, &request.tab_id, parent_tab_id)?;
        }
        AgentPlacement::Manager {
            parent_tab_id,
            role_profile,
        } => {
            mux.set_tab_role(&request.tab_id, AgentRole::Manager, Some(parent_tab_id))?;
            let tab = mux
                .get_tab_mut(&request.tab_id)
                .ok_or_else(|| format!("Tab {} not found", request.tab_id))?;
            tab.role_profile = role_profile.clone();
            tab.report_target_tab_id = Some(parent_tab_id.clone());
        }
        AgentPlacement::Planner { plan_id } => {
            let owner = rebind_owner_tab(plan_rebind, plan_id)?;
            mux.set_tab_role(&request.tab_id, AgentRole::Manager, Some(&owner))?;
            let tab = mux
                .get_tab_mut(&request.tab_id)
                .ok_or_else(|| format!("Tab {} not found", request.tab_id))?;
            tab.role_profile = Some("planner".into());
            tab.report_target_tab_id = Some(owner);
        }
        AgentPlacement::Orchestrator { can_spawn } => {
            mux.set_tab_role(&request.tab_id, AgentRole::Orchestrator, None)?;
            mux.set_can_spawn(&request.tab_id, *can_spawn)?;
        }
    }
    Ok(affected)
}

fn clear_lifecycle(mux: &mut MultiplexerState, tab_id: &str) -> Result<Vec<String>, String> {
    let mut affected = vec![tab_id.to_string()];
    let mut cursor = 0;
    while cursor < affected.len() {
        let children = mux.get_children_ids(&affected[cursor]);
        for child in children {
            if !affected.contains(&child) {
                affected.push(child);
            }
        }
        cursor += 1;
    }
    let old_teams: Vec<_> = affected
        .iter()
        .filter_map(|id| mux.get_tab(id).and_then(|tab| tab.team_id.clone()))
        .collect();
    mux.clear_tab_role(tab_id)?;
    for id in &affected {
        if let Some(tab) = mux.get_tab_mut(id) {
            tab.team_id = None;
            tab.pending_paired_planner_tab_id = None;
            tab.report_target_tab_id = None;
            tab.report_target_mesh_peer = None;
            tab.spawned_by_tab_id = None;
        }
    }
    for team_id in old_teams {
        let remains_in_use = mux
            .snapshot()
            .tabs
            .values()
            .any(|tab| tab.team_id.as_deref() == Some(team_id.as_str()));
        if !remains_in_use {
            mux.remove_team(&team_id);
        }
    }
    Ok(affected)
}

fn revocation_change(tab_id: String) -> AgentLifecyclePlanChange {
    AgentLifecyclePlanChange {
        tab_id,
        preserve_owned_plans: false,
        preserve_task_bindings_for_owner: None,
        planner_binding: None,
        plan_rebind: None,
    }
}

fn normalize_orchestrator(mux: &mut MultiplexerState, tab_id: &str) -> Result<(), String> {
    let tab = mux
        .get_tab_mut(tab_id)
        .ok_or_else(|| format!("Tab {tab_id} not found"))?;
    tab.role = Some(AgentRole::Orchestrator);
    tab.role_profile = None;
    tab.parent_tab_id = None;
    tab.can_spawn = true;
    tab.depth = 1;
    tab.report_target_tab_id = None;
    tab.report_target_mesh_peer = None;
    tab.spawned_by_tab_id = None;
    tab.pending_paired_planner_tab_id = None;
    Ok(())
}

fn set_report_owner(
    mux: &mut MultiplexerState,
    tab_id: &str,
    owner_tab_id: &str,
) -> Result<(), String> {
    let tab = mux
        .get_tab_mut(tab_id)
        .ok_or_else(|| format!("Tab {tab_id} not found"))?;
    tab.role_profile = None;
    tab.report_target_tab_id = Some(owner_tab_id.to_string());
    Ok(())
}

fn verified_session(tab: &Tab) -> Result<(&str, &str), String> {
    let session_id = tab
        .agent_session_id
        .as_deref()
        .ok_or_else(|| "agent_unavailable: live session binding is missing".to_string())?;
    let configured = tab
        .session_binding_nonce
        .as_deref()
        .ok_or_else(|| "agent_unavailable: session binding nonce is missing".to_string())?;
    let reported = tab
        .reported_session_binding_nonce
        .as_deref()
        .ok_or_else(|| "agent_unavailable: session binding nonce is unverified".to_string())?;
    if configured != reported {
        return Err("agent_unavailable: stale or foreign session binding nonce".into());
    }
    Ok((session_id, reported))
}

fn plan_owner(
    plan_store: &crate::service::plan::PlanStore,
    plan_id: &str,
) -> Result<String, String> {
    plan_store
        .get(plan_id)
        .and_then(|plan| plan.owner.tab_id)
        .filter(|tab_id| !tab_id.trim().is_empty())
        .ok_or_else(|| format!("Plan {plan_id} has no live Orchestrator owner"))
}

fn destination_plan_rebind(
    mux: &MultiplexerState,
    plan_store: &crate::service::plan::PlanStore,
    request: &AgentTransitionRequest,
    source: &Tab,
) -> Result<Option<AgentLifecyclePlanRebind>, String> {
    if let Some(rebind) = &request.plan_rebind {
        if !matches!(request.destination, AgentPlacement::Orchestrator { .. }) {
            return Err("plan rebind requires an Orchestrator destination".into());
        }
        verify_live_rebind_destination(mux, &request.tab_id, rebind)?;
        verify_rebind_team_available(plan_store, rebind)?;
        return Ok(Some(rebind.clone()));
    }
    let AgentPlacement::Planner { plan_id } = &request.destination else {
        return Ok(None);
    };
    let placed_owner = source
        .parent_tab_id
        .as_deref()
        .filter(|tab_id| {
            mux.get_tab(tab_id)
                .is_some_and(|tab| tab.role == Some(AgentRole::Orchestrator))
        })
        .map(str::to_string);
    let owner_tab_id = match placed_owner {
        Some(owner) => owner,
        None => plan_owner(plan_store, plan_id)?,
    };
    let owner = orchestrator_ref(mux, &owner_tab_id)?;
    let team_id = live_orchestrator_team(mux, &owner_tab_id)?;
    let rebind = AgentLifecyclePlanRebind {
        plan_id: plan_id.clone(),
        owner,
        team_id,
        reason: "Planner lifecycle rebind".into(),
        activate: false,
        clear_planner: false,
    };
    verify_rebind_team_available(plan_store, &rebind)?;
    Ok(Some(rebind))
}

fn verify_live_rebind_destination(
    mux: &MultiplexerState,
    tab_id: &str,
    rebind: &AgentLifecyclePlanRebind,
) -> Result<(), String> {
    if rebind.owner.tab_id.as_deref() != Some(tab_id) {
        return Err("plan rebind owner does not match the destination tab".into());
    }
    let team_id = live_orchestrator_team(mux, tab_id)?;
    if team_id != rebind.team_id {
        return Err("plan rebind Team does not match the destination Orchestrator".into());
    }
    Ok(())
}

fn verify_rebind_team_available(
    plan_store: &crate::service::plan::PlanStore,
    rebind: &AgentLifecyclePlanRebind,
) -> Result<(), String> {
    if let Some((existing_plan_id, existing_title)) =
        plan_store.active_plan_for_team(&rebind.team_id)
    {
        if existing_plan_id != rebind.plan_id {
            return Err(crate::service::plan::PlanError::TeamAlreadyHasActivePlan {
                team_id: rebind.team_id.clone(),
                existing_plan_id,
                existing_title,
            }
            .to_string());
        }
    }
    Ok(())
}

fn orchestrator_ref(
    mux: &MultiplexerState,
    tab_id: &str,
) -> Result<crate::plan::OrchestratorRef, String> {
    let tab = mux
        .get_tab(tab_id)
        .filter(|tab| tab.role == Some(AgentRole::Orchestrator))
        .ok_or_else(|| format!("Destination tab {tab_id} is not an Orchestrator"))?;
    Ok(crate::plan::OrchestratorRef {
        tab_id: Some(tab_id.to_string()),
        session_id: tab.agent_session_id.clone(),
        title: Some(tab.title.clone()),
    })
}

fn live_orchestrator_team(mux: &MultiplexerState, tab_id: &str) -> Result<String, String> {
    let tab = mux
        .get_tab(tab_id)
        .filter(|tab| tab.role == Some(AgentRole::Orchestrator))
        .ok_or_else(|| format!("Destination tab {tab_id} is not an Orchestrator"))?;
    tab.team_id
        .as_deref()
        .filter(|team_id| mux.get_team(team_id).is_some())
        .map(str::to_string)
        .ok_or_else(|| format!("Destination Orchestrator {tab_id} does not belong to a live Team"))
}

fn rebind_owner_tab(
    rebind: Option<&AgentLifecyclePlanRebind>,
    plan_id: &str,
) -> Result<String, String> {
    rebind
        .filter(|rebind| rebind.plan_id == plan_id)
        .and_then(|rebind| rebind.owner.tab_id.clone())
        .ok_or_else(|| format!("Plan {plan_id} has no lifecycle rebind destination"))
}

fn relink_plan_members(
    mux: &mut MultiplexerState,
    plan_store: &crate::service::plan::PlanStore,
    rebind: Option<&AgentLifecyclePlanRebind>,
) -> Result<(), String> {
    let Some(rebind) = rebind else {
        return Ok(());
    };
    let owner_tab_id = rebind
        .owner
        .tab_id
        .as_deref()
        .ok_or_else(|| "plan rebind requires a destination Orchestrator tab".to_string())?;
    let mut plan = plan_store
        .get(&rebind.plan_id)
        .ok_or_else(|| format!("Plan {} not found", rebind.plan_id))?;
    plan.planner = None;
    crate::service::plan::relink_to_owner(
        &mut crate::service::plan::lane_port_impl::LaneStatePort(mux),
        &plan,
        owner_tab_id,
    );
    Ok(())
}

fn planner_binding(
    mux: &MultiplexerState,
    request: &AgentTransitionRequest,
    plan_rebind: Option<&AgentLifecyclePlanRebind>,
) -> Result<Option<(String, Option<PlannerRef>)>, String> {
    let AgentPlacement::Planner { plan_id } = &request.destination else {
        return Ok(None);
    };
    let tab = mux
        .get_tab(&request.tab_id)
        .ok_or_else(|| format!("Tab {} not found", request.tab_id))?;
    let (session_id, _) = verified_session(tab)?;
    let owner = rebind_owner_tab(plan_rebind, plan_id)?;
    if tab.parent_tab_id.as_deref() != Some(owner.as_str()) {
        return Err("planner parent does not match the destination plan owner".into());
    }
    Ok(Some((
        plan_id.clone(),
        Some(PlannerRef::Tab {
            tab_id: request.tab_id.clone(),
            session_id: Some(session_id.to_string()),
            title: Some(tab.title.clone()),
        }),
    )))
}

fn planner_incumbent(
    plan_store: &crate::service::plan::PlanStore,
    destination: &AgentPlacement,
    plan_rebind: Option<&AgentLifecyclePlanRebind>,
    target_tab_id: &str,
) -> Option<String> {
    let plan_id = match destination {
        AgentPlacement::Planner { plan_id } => plan_id,
        AgentPlacement::Orchestrator { .. } => {
            let rebind = plan_rebind.filter(|rebind| rebind.clear_planner)?;
            &rebind.plan_id
        }
        _ => return None,
    };
    let Some(PlannerRef::Tab { tab_id, .. }) = plan_store.get(plan_id)?.planner else {
        return None;
    };
    (tab_id != target_tab_id).then_some(tab_id)
}

fn verify_destination(
    mux: &MultiplexerState,
    request: &AgentTransitionRequest,
    plan_rebind: Option<&AgentLifecyclePlanRebind>,
) -> Result<(), String> {
    let tab = mux
        .get_tab(&request.tab_id)
        .ok_or_else(|| format!("Tab {} not found", request.tab_id))?;
    match &request.destination {
        AgentPlacement::Roleless => {
            if tab.role.is_some()
                || tab.parent_tab_id.is_some()
                || tab.team_id.is_some()
                || tab.can_spawn
                || tab.report_target_tab_id.is_some()
            {
                return Err("roleless transition post-condition failed".into());
            }
        }
        AgentPlacement::Worker { parent_tab_id } => {
            verify_leaf(tab, AgentRole::Worker, None, parent_tab_id, mux)?
        }
        AgentPlacement::Manager {
            parent_tab_id,
            role_profile,
        } => verify_leaf(
            tab,
            AgentRole::Manager,
            role_profile.as_deref(),
            parent_tab_id,
            mux,
        )?,
        AgentPlacement::Planner { plan_id } => {
            let owner = rebind_owner_tab(plan_rebind, plan_id)?;
            verify_leaf(tab, AgentRole::Manager, Some("planner"), &owner, mux)?;
            verified_session(tab)?;
        }
        AgentPlacement::Orchestrator { can_spawn } => {
            if tab.role != Some(AgentRole::Orchestrator)
                || tab.parent_tab_id.is_some()
                || tab.depth != 1
                || tab.can_spawn != *can_spawn
                || tab.report_target_tab_id.is_some()
            {
                return Err("Orchestrator transition post-condition failed".into());
            }
        }
    }
    Ok(())
}

fn verify_leaf(
    tab: &Tab,
    role: AgentRole,
    role_profile: Option<&str>,
    parent_tab_id: &str,
    mux: &MultiplexerState,
) -> Result<(), String> {
    let parent_team = mux
        .get_tab(parent_tab_id)
        .and_then(|parent| parent.team_id.as_deref());
    if tab.role != Some(role)
        || tab.role_profile.as_deref() != role_profile
        || tab.parent_tab_id.as_deref() != Some(parent_tab_id)
        || tab.team_id.as_deref() != parent_team
        || tab.can_spawn
        || tab.depth != 2
        || tab.report_target_tab_id.as_deref() != Some(parent_tab_id)
    {
        return Err("leaf transition post-condition failed".into());
    }
    Ok(())
}

fn verify_persisted_plan_rebind(
    plan_store: &crate::service::plan::PlanStore,
    rebind: &AgentLifecyclePlanRebind,
) -> Result<crate::plan::Plan, String> {
    let plan = plan_store
        .get(&rebind.plan_id)
        .ok_or_else(|| format!("Plan {} not found after lifecycle rebind", rebind.plan_id))?;
    if plan.owner != rebind.owner || plan.team_id.as_deref() != Some(rebind.team_id.as_str()) {
        return Err(format!(
            "plan lifecycle rebind post-condition failed for {}",
            rebind.plan_id
        ));
    }
    if rebind.activate && plan.status != crate::plan::PlanStatus::Active {
        return Err(format!(
            "plan lifecycle rebind did not activate {}",
            rebind.plan_id
        ));
    }
    Ok(plan)
}

fn compensate_injected_failure(
    plan_store: &crate::service::plan::PlanStore,
    transaction: AgentLifecyclePlanTransaction,
) -> Result<Vec<String>, String> {
    plan_store
        .rollback_agent_lifecycle_change(transaction)
        .map_err(|error| format!("transition failed and compensation failed: {error}"))?;
    Err("injected agent transition failure".into())
}

fn compensate_failed_postcondition(
    plan_store: &crate::service::plan::PlanStore,
    transaction: AgentLifecyclePlanTransaction,
    error: String,
) -> Result<Vec<String>, String> {
    plan_store
        .rollback_agent_lifecycle_change(transaction)
        .map_err(|rollback| format!("{error}; compensation failed: {rollback}"))?;
    Err(error)
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
