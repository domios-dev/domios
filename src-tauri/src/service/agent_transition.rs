use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{AgentRole, Pane};
use crate::plan::PlannerRef;
use crate::service::plan::mutations::agent_lifecycle::{
    AgentLifecyclePlanChange, AgentLifecyclePlanTransaction,
};
use crate::service::ServiceContext;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentPlacement {
    Roleless,
    Worker {
        parent_pane_id: String,
    },
    Planner {
        plan_id: String,
    },
    Manager {
        parent_pane_id: String,
        role_profile: Option<String>,
    },
    Orchestrator {
        can_spawn: bool,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct AgentTransitionRequest {
    pub pane_id: String,
    pub destination: AgentPlacement,
    pub require_verified_session: bool,
    pub planner_replacement: Option<(String, Option<PlannerRef>)>,
}

impl AgentTransitionRequest {
    pub(crate) fn roleless(pane_id: impl Into<String>) -> Self {
        Self {
            pane_id: pane_id.into(),
            destination: AgentPlacement::Roleless,
            require_verified_session: false,
            planner_replacement: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransitionFailurePoint {
    None,
    AfterPlanCommit,
}

struct PreparedTransition {
    staged_mux: crate::multiplexer::state::TeamCloseTransaction,
    plan_changes: Vec<AgentLifecyclePlanChange>,
}

impl ServiceContext {
    pub fn set_pane_role(
        &self,
        pane_id: &str,
        role_str: &str,
        parent_pane_id: Option<&str>,
        can_spawn: Option<bool>,
    ) -> Result<(), String> {
        let destination = match Self::parse_role(role_str)? {
            AgentRole::Orchestrator => AgentPlacement::Orchestrator {
                can_spawn: can_spawn.unwrap_or(true),
            },
            AgentRole::Manager => AgentPlacement::Manager {
                parent_pane_id: parent_pane_id
                    .ok_or("Manager requires a parent_pane_id")?
                    .to_string(),
                role_profile: None,
            },
            AgentRole::Worker => AgentPlacement::Worker {
                parent_pane_id: parent_pane_id
                    .ok_or("Worker requires a parent_pane_id")?
                    .to_string(),
            },
        };
        self.transition_agent(AgentTransitionRequest {
            pane_id: pane_id.to_string(),
            destination,
            require_verified_session: false,
            planner_replacement: None,
        })?;
        Ok(())
    }

    pub fn clear_pane_role(&self, pane_id: &str) -> Result<(), String> {
        match self.transition_agent(AgentTransitionRequest::roleless(pane_id)) {
            Ok(_) => Ok(()),
            Err(error) if error.contains("not found") => {
                let touched = self
                    .remote_workers
                    .forget_by_remote_pane(pane_id, None, None);
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
        if let Some(PlannerRef::Pane { pane_id, .. }) = &planner {
            return self.transition_agent(AgentTransitionRequest {
                pane_id: pane_id.clone(),
                destination: AgentPlacement::Planner {
                    plan_id: plan_id.to_string(),
                },
                require_verified_session: true,
                planner_replacement: None,
            });
        }

        let current = self
            .plan_store
            .get(plan_id)
            .ok_or_else(|| format!("Plan {plan_id} not found"))?
            .planner;
        let old_pane_id = match current {
            Some(PlannerRef::Pane { pane_id, .. }) => Some(pane_id),
            _ => None,
        };
        if let Some(pane_id) = old_pane_id.as_ref() {
            if self.mux.read().get_pane(&pane_id).is_some() {
                return self.transition_agent(AgentTransitionRequest {
                    pane_id: pane_id.clone(),
                    destination: AgentPlacement::Roleless,
                    require_verified_session: false,
                    planner_replacement: Some((plan_id.to_string(), planner)),
                });
            }
        }

        let mux = self.mux.write();
        let change = AgentLifecyclePlanChange {
            pane_id: old_pane_id.unwrap_or_default(),
            preserve_owned_plans: true,
            preserve_task_bindings_for_owner: None,
            planner_binding: Some((plan_id.to_string(), planner)),
            planner_team_binding: None,
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

fn execute_transition(
    mux: &mut MultiplexerState,
    plan_store: &crate::service::plan::PlanStore,
    request: AgentTransitionRequest,
    failure: TransitionFailurePoint,
) -> Result<Vec<String>, String> {
    let prepared = prepare_transition(mux, plan_store, &request)?;
    let plan_transaction = plan_store
        .apply_agent_lifecycle_changes(prepared.plan_changes, epoch_ms())
        .map_err(|error| error.to_string())?;
    if failure == TransitionFailurePoint::AfterPlanCommit {
        return compensate_injected_failure(plan_store, plan_transaction);
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
        .get_pane(&request.pane_id)
        .ok_or_else(|| format!("Pane {} not found", request.pane_id))?
        .clone();
    if request.require_verified_session {
        verified_session(&source)?;
    }

    let mut staged_mux = mux.begin_team_close_transaction();
    let staged = staged_mux.state_mut();
    let mut plan_changes = Vec::new();
    let preserve_owned_plans = matches!(request.destination, AgentPlacement::Orchestrator { .. })
        && source.role == Some(AgentRole::Orchestrator)
        && source.parent_pane_id.is_none();
    let preserve_task_bindings_for_owner = match &request.destination {
        AgentPlacement::Worker { parent_pane_id }
            if source.role == Some(AgentRole::Worker)
                && source.parent_pane_id.as_deref() == Some(parent_pane_id.as_str()) =>
        {
            Some(parent_pane_id.clone())
        }
        _ => None,
    };

    let incumbent = planner_incumbent(plan_store, &request.destination, &request.pane_id);
    if let Some(incumbent_id) = incumbent.as_deref() {
        for affected_id in clear_lifecycle(staged, incumbent_id)? {
            plan_changes.push(revocation_change(affected_id));
        }
    }

    let affected = apply_destination(staged, plan_store, request, preserve_owned_plans)?;
    for affected_id in affected
        .into_iter()
        .filter(|pane_id| pane_id != &request.pane_id)
    {
        plan_changes.push(revocation_change(affected_id));
    }
    let planner_binding = planner_binding(staged, plan_store, request)?;
    let planner_team_binding = planner_team_binding(staged, request)?;
    plan_changes.push(AgentLifecyclePlanChange {
        pane_id: request.pane_id.clone(),
        preserve_owned_plans,
        preserve_task_bindings_for_owner,
        planner_binding: request.planner_replacement.clone().or(planner_binding),
        planner_team_binding,
    });
    verify_destination(staged, plan_store, request)?;

    Ok(PreparedTransition {
        staged_mux,
        plan_changes,
    })
}

fn apply_destination(
    mux: &mut MultiplexerState,
    plan_store: &crate::service::plan::PlanStore,
    request: &AgentTransitionRequest,
    preserve_owned_plans: bool,
) -> Result<Vec<String>, String> {
    if preserve_owned_plans {
        normalize_orchestrator(mux, &request.pane_id)?;
        return Ok(Vec::new());
    }

    let affected = clear_lifecycle(mux, &request.pane_id)?;
    match &request.destination {
        AgentPlacement::Roleless => {}
        AgentPlacement::Worker { parent_pane_id } => {
            mux.set_pane_role(&request.pane_id, AgentRole::Worker, Some(parent_pane_id))?;
            set_report_owner(mux, &request.pane_id, parent_pane_id)?;
        }
        AgentPlacement::Manager {
            parent_pane_id,
            role_profile,
        } => {
            mux.set_pane_role(&request.pane_id, AgentRole::Manager, Some(parent_pane_id))?;
            let pane = mux
                .get_pane_mut(&request.pane_id)
                .ok_or_else(|| format!("Pane {} not found", request.pane_id))?;
            pane.role_profile = role_profile.clone();
            pane.report_target_pane_id = Some(parent_pane_id.clone());
        }
        AgentPlacement::Planner { plan_id } => {
            let owner = plan_owner(plan_store, plan_id)?;
            mux.set_pane_role(&request.pane_id, AgentRole::Manager, Some(&owner))?;
            let pane = mux
                .get_pane_mut(&request.pane_id)
                .ok_or_else(|| format!("Pane {} not found", request.pane_id))?;
            pane.role_profile = Some("planner".into());
            pane.report_target_pane_id = Some(owner);
        }
        AgentPlacement::Orchestrator { can_spawn } => {
            mux.set_pane_role(&request.pane_id, AgentRole::Orchestrator, None)?;
            mux.set_can_spawn(&request.pane_id, *can_spawn)?;
        }
    }
    Ok(affected)
}

fn clear_lifecycle(mux: &mut MultiplexerState, pane_id: &str) -> Result<Vec<String>, String> {
    let mut affected = vec![pane_id.to_string()];
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
        .filter_map(|id| mux.get_pane(id).and_then(|pane| pane.team_id.clone()))
        .collect();
    mux.clear_pane_role(pane_id)?;
    for id in &affected {
        if let Some(pane) = mux.get_pane_mut(id) {
            pane.team_id = None;
            pane.pending_paired_planner_pane_id = None;
            pane.report_target_pane_id = None;
            pane.report_target_mesh_peer = None;
            pane.spawned_by_pane_id = None;
        }
    }
    for team_id in old_teams {
        let remains_in_use = mux
            .snapshot()
            .panes
            .values()
            .any(|pane| pane.team_id.as_deref() == Some(team_id.as_str()));
        if !remains_in_use {
            mux.remove_team(&team_id);
        }
    }
    Ok(affected)
}

fn revocation_change(pane_id: String) -> AgentLifecyclePlanChange {
    AgentLifecyclePlanChange {
        pane_id,
        preserve_owned_plans: false,
        preserve_task_bindings_for_owner: None,
        planner_binding: None,
        planner_team_binding: None,
    }
}

fn normalize_orchestrator(mux: &mut MultiplexerState, pane_id: &str) -> Result<(), String> {
    let pane = mux
        .get_pane_mut(pane_id)
        .ok_or_else(|| format!("Pane {pane_id} not found"))?;
    pane.role = Some(AgentRole::Orchestrator);
    pane.role_profile = None;
    pane.parent_pane_id = None;
    pane.can_spawn = true;
    pane.depth = 1;
    pane.report_target_pane_id = None;
    pane.report_target_mesh_peer = None;
    pane.spawned_by_pane_id = None;
    pane.pending_paired_planner_pane_id = None;
    Ok(())
}

fn set_report_owner(
    mux: &mut MultiplexerState,
    pane_id: &str,
    owner_pane_id: &str,
) -> Result<(), String> {
    let pane = mux
        .get_pane_mut(pane_id)
        .ok_or_else(|| format!("Pane {pane_id} not found"))?;
    pane.role_profile = None;
    pane.report_target_pane_id = Some(owner_pane_id.to_string());
    Ok(())
}

fn verified_session(pane: &Pane) -> Result<(&str, &str), String> {
    let session_id = pane
        .agent_session_id
        .as_deref()
        .ok_or_else(|| "agent_unavailable: live session binding is missing".to_string())?;
    let configured = pane
        .session_binding_nonce
        .as_deref()
        .ok_or_else(|| "agent_unavailable: session binding nonce is missing".to_string())?;
    let reported = pane
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
        .and_then(|plan| plan.owner.pane_id)
        .filter(|pane_id| !pane_id.trim().is_empty())
        .ok_or_else(|| format!("Plan {plan_id} has no live Orchestrator owner"))
}

fn planner_binding(
    mux: &MultiplexerState,
    plan_store: &crate::service::plan::PlanStore,
    request: &AgentTransitionRequest,
) -> Result<Option<(String, Option<PlannerRef>)>, String> {
    let AgentPlacement::Planner { plan_id } = &request.destination else {
        return Ok(None);
    };
    let pane = mux
        .get_pane(&request.pane_id)
        .ok_or_else(|| format!("Pane {} not found", request.pane_id))?;
    let (session_id, _) = verified_session(pane)?;
    let owner = plan_owner(plan_store, plan_id)?;
    if pane.parent_pane_id.as_deref() != Some(owner.as_str()) {
        return Err("planner parent does not match the current plan owner".into());
    }
    Ok(Some((
        plan_id.clone(),
        Some(PlannerRef::Pane {
            pane_id: request.pane_id.clone(),
            session_id: Some(session_id.to_string()),
            title: Some(pane.title.clone()),
        }),
    )))
}

fn planner_team_binding(
    mux: &MultiplexerState,
    request: &AgentTransitionRequest,
) -> Result<Option<(String, String)>, String> {
    let AgentPlacement::Planner { plan_id } = &request.destination else {
        return Ok(None);
    };
    let team_id = mux
        .get_pane(&request.pane_id)
        .and_then(|pane| pane.team_id.clone())
        .ok_or_else(|| "Planner transition did not establish Team membership".to_string())?;
    Ok(Some((plan_id.clone(), team_id)))
}

fn planner_incumbent(
    plan_store: &crate::service::plan::PlanStore,
    destination: &AgentPlacement,
    target_pane_id: &str,
) -> Option<String> {
    let AgentPlacement::Planner { plan_id } = destination else {
        return None;
    };
    let Some(PlannerRef::Pane { pane_id, .. }) = plan_store.get(plan_id)?.planner else {
        return None;
    };
    (pane_id != target_pane_id).then_some(pane_id)
}

fn verify_destination(
    mux: &MultiplexerState,
    plan_store: &crate::service::plan::PlanStore,
    request: &AgentTransitionRequest,
) -> Result<(), String> {
    let pane = mux
        .get_pane(&request.pane_id)
        .ok_or_else(|| format!("Pane {} not found", request.pane_id))?;
    match &request.destination {
        AgentPlacement::Roleless => {
            if pane.role.is_some()
                || pane.parent_pane_id.is_some()
                || pane.team_id.is_some()
                || pane.can_spawn
                || pane.report_target_pane_id.is_some()
            {
                return Err("roleless transition post-condition failed".into());
            }
        }
        AgentPlacement::Worker { parent_pane_id } => {
            verify_leaf(pane, AgentRole::Worker, None, parent_pane_id, mux)?
        }
        AgentPlacement::Manager {
            parent_pane_id,
            role_profile,
        } => verify_leaf(
            pane,
            AgentRole::Manager,
            role_profile.as_deref(),
            parent_pane_id,
            mux,
        )?,
        AgentPlacement::Planner { plan_id } => {
            let owner = plan_owner(plan_store, plan_id)?;
            verify_leaf(pane, AgentRole::Manager, Some("planner"), &owner, mux)?;
            verified_session(pane)?;
        }
        AgentPlacement::Orchestrator { can_spawn } => {
            if pane.role != Some(AgentRole::Orchestrator)
                || pane.parent_pane_id.is_some()
                || pane.depth != 1
                || pane.can_spawn != *can_spawn
                || pane.report_target_pane_id.is_some()
            {
                return Err("Orchestrator transition post-condition failed".into());
            }
        }
    }
    Ok(())
}

fn verify_leaf(
    pane: &Pane,
    role: AgentRole,
    role_profile: Option<&str>,
    parent_pane_id: &str,
    mux: &MultiplexerState,
) -> Result<(), String> {
    let parent_team = mux
        .get_pane(parent_pane_id)
        .and_then(|parent| parent.team_id.as_deref());
    if pane.role != Some(role)
        || pane.role_profile.as_deref() != role_profile
        || pane.parent_pane_id.as_deref() != Some(parent_pane_id)
        || pane.team_id.as_deref() != parent_team
        || pane.can_spawn
        || pane.depth != 2
        || pane.report_target_pane_id.as_deref() != Some(parent_pane_id)
    {
        return Err("leaf transition post-condition failed".into());
    }
    Ok(())
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

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
