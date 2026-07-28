use codeterm_types::team::Team;

use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{AgentRole, TabBacking};
use crate::service::plan::PlanError;
use crate::service::ServiceContext;

#[derive(Debug, Clone)]
pub(crate) struct AddTeamMemberParams {
    pub provider: String,
    pub role: String,
    pub title: Option<String>,
    pub task: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct AddTeamMemberResult {
    pub tab_id: String,
    pub team: Team,
}

#[derive(Debug)]
pub(crate) enum TeamPlanError {
    Team(String),
    Plan(PlanError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TeamMutationCaller {
    TrustedDirectUser,
    AgentTab(String),
}

impl ServiceContext {
    pub(crate) fn reconcile_dangling_team_plan_bindings(
        &self,
        now_ms: u64,
    ) -> Result<Vec<String>, crate::service::plan::PlanError> {
        let changed_plan_ids = {
            let mux = self.mux.read();
            reconcile_dangling_team_plan_bindings(&mux, &self.plan_store, now_ms)?
        };
        for plan_id in &changed_plan_ids {
            self.emit_plan_changed(plan_id);
        }
        Ok(changed_plan_ids)
    }

    pub(crate) async fn add_team_member(
        &self,
        team_id: &str,
        caller_tab_id: &str,
        params: AddTeamMemberParams,
    ) -> Result<AddTeamMemberResult, String> {
        let prepared = {
            let mux = self.mux.read();
            prepare_team_member_spawn_with_provider_validation(
                &mux,
                team_id,
                caller_tab_id,
                params,
                |spawn_params| {
                    self.classify_spawn_target_for_params(
                        spawn_params.agent.as_deref().unwrap_or_default(),
                        spawn_params,
                    )
                    .map(|_| ())
                },
            )?
        };
        let mut spawn_params = prepared.spawn_params;
        self.attach_manager_bootstrap(team_id, caller_tab_id, &mut spawn_params)?;
        let lane_label = format!("team-{team_id}");
        let service = self.clone();
        let (spawn_params, mut lane) = tauri::async_runtime::spawn_blocking(move || {
            let lane = service.provision_worker_lane_for_spawn(&mut spawn_params, &lane_label)?;
            Ok::<_, String>((spawn_params, lane))
        })
        .await
        .map_err(|error| format!("worker lane provisioning failed: {error}"))??;
        let spawn = self
            .spawn_agent_with_state_change(spawn_params, None, false)
            .await;
        let tab_id = match spawn {
            Ok(spawn) => spawn.tab_id,
            Err(error) => {
                if let Some(lane) = lane.take() {
                    let _ = tauri::async_runtime::spawn_blocking(move || lane.cleanup()).await;
                }
                return Err(error);
            }
        };
        let result = {
            let mut mux = self.mux.write();
            commit_team_member_spawn(
                &mut mux,
                &self.events,
                team_id,
                caller_tab_id,
                &tab_id,
                &prepared.expected_role,
                &prepared.expected_profile,
            )
        };
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                if let Some(lane) = lane.take() {
                    let _ = tauri::async_runtime::spawn_blocking(move || lane.cleanup()).await;
                }
                return Err(error);
            }
        };
        self.emit_state_changed_after_bus_publish();
        Ok(result)
    }

    fn attach_manager_bootstrap(
        &self,
        team_id: &str,
        caller_tab_id: &str,
        params: &mut crate::service::SpawnAgentParams,
    ) -> Result<(), String> {
        let role = match (params.role.as_deref(), params.role_profile.as_deref()) {
            (Some("Orchestrator"), _) => "Orchestrator",
            (Some("Manager"), Some("planner")) => "Planner",
            _ => return Ok(()),
        };
        let (team_name, project) = {
            let mux = self.mux.read();
            let team = mux
                .get_team(team_id)
                .ok_or_else(|| format!("Team {team_id} not found"))?;
            let project = mux
                .get_workspace(&team.workspace_id)
                .map(|workspace| workspace.name.clone())
                .unwrap_or_else(|| team.workspace_id.clone());
            (
                team.name.clone().unwrap_or_else(|| "Unnamed Team".into()),
                project,
            )
        };
        let plan_id = self
            .plan_store
            .list_full()
            .into_iter()
            .find(|plan| plan.team_id.as_deref() == Some(team_id))
            .map(|plan| plan.id)
            .unwrap_or_else(|| "no bound plan yet".into());
        let coordination = (role == "Planner").then(|| {
            let mux = self.mux.read();
            let caller_label = mux.display_label_for_tab(caller_tab_id);
            let planner = crate::service::spawn_factory::Counterpart::new("Planner", "", "");
            let orchestrator = crate::service::spawn_factory::Counterpart::new(
                "Orchestrator",
                caller_label,
                caller_tab_id,
            );
            crate::service::spawn_factory::introduction_body(&planner, &orchestrator)
        });
        params.manager_bootstrap = Some(crate::service::role_identity::ManagerBootstrapInputs {
            role: role.into(),
            team_id: team_id.into(),
            team_name,
            plan_id,
            project,
            coordination,
        });
        Ok(())
    }

    pub(crate) fn rename_team(
        &self,
        team_id: &str,
        name: String,
        caller_tab_id: Option<&str>,
    ) -> Result<Team, String> {
        let mut mux = self.mux.write();
        let team =
            rename_team_with_state_change(&mut mux, &self.events, team_id, name, caller_tab_id)?;
        drop(mux);
        self.emit_state_changed_after_bus_publish();
        Ok(team)
    }

    pub(crate) fn close_team(
        &self,
        team_id: &str,
        caller_tab_id: Option<&str>,
    ) -> Result<Team, String> {
        let (team, cleanup, changed_plan_ids) = {
            let mut mux = self.mux.write();
            close_team_with_state_change(
                &mut mux,
                &self.events,
                &self.plan_store,
                team_id,
                caller_tab_id,
                now_ms(),
            )?
        };
        for plan_id in changed_plan_ids {
            self.emit_plan_changed(&plan_id);
        }
        for tab_id in &cleanup.member_tab_ids {
            self.chat_registry.shutdown_tab(tab_id);
        }
        for (plugin_id, session_id) in &cleanup.chat_backings {
            let _ = self.chatbackend_close(plugin_id, session_id);
        }
        for tab_id in &cleanup.member_tab_ids {
            self.flag_bound_tasks_for_gone_tab(tab_id, 0);
            self.prompt_detector.lock().remove_tab(tab_id);
            crate::service::structured_prompt::clear_tab(tab_id);
            self.remote_workers.forget_orchestrator(tab_id);
        }
        self.kill_ptys(&cleanup.pty_ids);
        self.emit_state_changed_after_bus_publish();
        Ok(team)
    }

    pub(crate) fn adopt_plan_for_team(
        &self,
        team_id: &str,
        plan_id: &str,
        caller: TeamMutationCaller,
        now_ms: u64,
    ) -> Result<crate::plan::Plan, TeamPlanError> {
        let mut mux = self.mux.write();
        let plan =
            adopt_plan_for_team_core(&mut mux, &self.plan_store, team_id, plan_id, caller, now_ms)?;
        drop(mux);
        crate::api::events::emit_everywhere(
            &self.app_handle,
            "plan-changed",
            serde_json::json!({ "plan_id": plan_id }),
        );
        self.emit_state_changed();
        Ok(plan)
    }

    pub(crate) fn resume_team_plan(
        &self,
        plan_id: &str,
        now_ms: u64,
    ) -> Result<crate::plan::PlanMode, TeamPlanError> {
        let mux = self.mux.read();
        self.valid_live_plan_team(&mux, plan_id)?;
        let mode = self
            .plan_store
            .resume_plan(plan_id, now_ms)
            .map_err(TeamPlanError::Plan)?;
        drop(mux);
        self.emit_plan_changed(plan_id);
        self.reconcile_time_reminder_eligibility();
        Ok(mode)
    }

    pub(crate) fn unarchive_team_plan(
        &self,
        plan_id: &str,
        now_ms: u64,
    ) -> Result<(), TeamPlanError> {
        let mux = self.mux.read();
        self.valid_live_plan_team(&mux, plan_id)?;
        self.plan_store
            .unarchive(plan_id, now_ms)
            .map_err(TeamPlanError::Plan)?;
        drop(mux);
        self.emit_plan_changed(plan_id);
        Ok(())
    }

    pub(crate) fn transfer_team_plan(
        &self,
        plan_id: &str,
        to: crate::plan::OrchestratorRef,
        reason: &str,
        caller_tab_id: String,
        now_ms: u64,
    ) -> Result<crate::plan::Plan, TeamPlanError> {
        let mut mux = self.mux.write();
        let plan = transfer_team_plan_core(
            &mut mux,
            &self.plan_store,
            plan_id,
            to,
            reason,
            TeamMutationCaller::AgentTab(caller_tab_id),
            now_ms,
        )?;
        drop(mux);
        self.emit_state_changed();
        self.emit_plan_changed(plan_id);
        Ok(plan)
    }

    pub(crate) fn validate_active_team_plan_bindings(&self) -> Result<(), TeamPlanError> {
        let mux = self.mux.read();
        let mut claimed_teams = std::collections::BTreeMap::<String, String>::new();
        for plan in self
            .plan_store
            .list_full()
            .into_iter()
            .filter(|plan| plan.status == crate::plan::PlanStatus::Active)
        {
            let team_id = self.valid_live_plan_team(&mux, &plan.id)?;
            if let Some(existing_plan_id) = claimed_teams.insert(team_id.clone(), plan.id.clone()) {
                return Err(TeamPlanError::Plan(PlanError::TeamAlreadyHasActivePlan {
                    team_id,
                    existing_plan_id,
                    existing_title: "conflicting active plan".into(),
                }));
            }
        }
        Ok(())
    }

    pub(crate) fn validate_plan_team_binding(&self, plan_id: &str) -> Result<(), TeamPlanError> {
        let Some(plan) = self.plan_store.get(plan_id) else {
            return Err(TeamPlanError::Plan(PlanError::NotFound));
        };
        if plan.status == crate::plan::PlanStatus::Archived {
            return Ok(());
        }
        let mux = self.mux.read();
        self.valid_live_plan_team(&mux, plan_id).map(|_| ())
    }

    fn valid_live_plan_team(
        &self,
        mux: &MultiplexerState,
        plan_id: &str,
    ) -> Result<String, TeamPlanError> {
        let plan = self
            .plan_store
            .get(plan_id)
            .ok_or(TeamPlanError::Plan(PlanError::NotFound))?;
        let team_id = plan
            .team_id
            .filter(|team_id| !team_id.trim().is_empty())
            .ok_or_else(|| {
                TeamPlanError::Plan(PlanError::ActivePlanRequiresTeam {
                    plan_id: plan_id.into(),
                })
            })?;
        if mux.get_team(&team_id).is_none() {
            return Err(TeamPlanError::Team(format!(
                "Plan {plan_id} references missing Team {team_id}; adopt it into a live Team before resuming"
            )));
        }
        Ok(team_id)
    }
}

fn adopt_plan_for_team_core(
    mux: &mut MultiplexerState,
    plan_store: &crate::service::plan::PlanStore,
    team_id: &str,
    plan_id: &str,
    caller: TeamMutationCaller,
    now_ms: u64,
) -> Result<crate::plan::Plan, TeamPlanError> {
    authorize_team_mutation(mux, team_id, caller).map_err(|error| {
        TeamPlanError::Team(format!(
            "Plan adoption requires manager authority in destination Team {team_id}: {error}. \
Safe recovery: old-team member transfers ownership, then new owner adopts."
        ))
    })?;
    let owner_tab_id = live_team_orchestrator(mux, team_id)?;
    crate::service::agent_transition::transition_plan_to_orchestrator_core(
        mux,
        plan_store,
        plan_id,
        crate::plan::OrchestratorRef::from_tab(&owner_tab_id),
        team_id,
        &format!("adopted by Team {team_id}"),
        true,
        now_ms,
    )
    .map_err(TeamPlanError::Team)
}

fn transfer_team_plan_core(
    mux: &mut MultiplexerState,
    plan_store: &crate::service::plan::PlanStore,
    plan_id: &str,
    to: crate::plan::OrchestratorRef,
    reason: &str,
    caller: TeamMutationCaller,
    now_ms: u64,
) -> Result<crate::plan::Plan, TeamPlanError> {
    let source_plan = plan_store
        .get(plan_id)
        .ok_or(TeamPlanError::Plan(PlanError::NotFound))?;
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
    if tab.role != Some(AgentRole::Orchestrator) {
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
    crate::service::agent_transition::transition_plan_to_orchestrator_core(
        mux,
        plan_store,
        plan_id,
        to,
        &destination_team_id,
        reason,
        false,
        now_ms,
    )
    .map_err(TeamPlanError::Team)
}

fn authorize_plan_transfer(
    mux: &MultiplexerState,
    source_team_id: &str,
    destination_team_id: &str,
    destination_owner_tab_id: &str,
    caller: TeamMutationCaller,
) -> Result<(), String> {
    if authorize_team_mutation(mux, source_team_id, caller.clone()).is_ok() {
        return Ok(());
    }
    match caller {
        TeamMutationCaller::TrustedDirectUser => Ok(()),
        TeamMutationCaller::AgentTab(tab_id) if tab_id == destination_owner_tab_id => {
            authorize_team_manager(mux, destination_team_id, Some(&tab_id))
        }
        TeamMutationCaller::AgentTab(tab_id) => Err(format!(
            "Caller tab {tab_id} lacks plan rebind authority: a source Team {source_team_id} \
manager or destination Orchestrator {destination_owner_tab_id} in Team \
{destination_team_id} must authorize the transfer. Safe recovery: old-team member \
transfers ownership, then new owner adopts."
        )),
    }
}

fn live_team_orchestrator(mux: &MultiplexerState, team_id: &str) -> Result<String, TeamPlanError> {
    let mut orchestrators: Vec<_> = mux
        .snapshot()
        .tabs
        .values()
        .filter(|tab| {
            tab.team_id.as_deref() == Some(team_id) && tab.role == Some(AgentRole::Orchestrator)
        })
        .map(|tab| tab.id.clone())
        .collect();
    orchestrators.sort();
    match orchestrators.as_slice() {
        [tab_id] => Ok(tab_id.clone()),
        [] => Err(TeamPlanError::Team(format!(
            "Team {team_id} has no live Orchestrator"
        ))),
        _ => Err(TeamPlanError::Team(format!(
            "Team {team_id} has multiple live Orchestrators; transfer to a specific tab instead"
        ))),
    }
}

#[derive(Debug)]
pub(crate) struct CloseTeamCleanup {
    member_tab_ids: Vec<String>,
    chat_backings: Vec<(String, String)>,
    pty_ids: Vec<String>,
}

#[derive(Debug)]
struct PreparedTeamMemberSpawn {
    spawn_params: crate::service::SpawnAgentParams,
    expected_role: String,
    expected_profile: Option<String>,
}

fn prepare_team_member_spawn(
    mux: &MultiplexerState,
    team_id: &str,
    caller_tab_id: &str,
    params: AddTeamMemberParams,
) -> Result<PreparedTeamMemberSpawn, String> {
    authorize_team_manager(mux, team_id, Some(caller_tab_id))?;
    let (role, role_profile) = team_member_role(&params.role)?;
    Ok(PreparedTeamMemberSpawn {
        expected_role: role.clone(),
        expected_profile: role_profile.clone(),
        spawn_params: crate::service::SpawnAgentParams {
            agent: Some(params.provider),
            title: params.title,
            role: Some(role),
            role_profile,
            parent_tab_id: Some(caller_tab_id.into()),
            task: params.task,
            model: params.model,
            spawned_by_tab_id: Some(caller_tab_id.into()),
            ..Default::default()
        },
    })
}

fn prepare_team_member_spawn_with_provider_validation<F>(
    mux: &MultiplexerState,
    team_id: &str,
    caller_tab_id: &str,
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
    caller_tab_id: &str,
    tab_id: &str,
    expected_role: &str,
    expected_profile: &Option<String>,
) -> Result<AddTeamMemberResult, String> {
    let tab = mux
        .get_tab(tab_id)
        .ok_or_else(|| format!("Tab {tab_id} not found after member spawn"))?;
    let invalid_hierarchy = tab.parent_tab_id.as_deref() != Some(caller_tab_id)
        || tab.team_id.as_deref() != Some(team_id)
        || tab.role != AgentRole::from_str_compat(expected_role)
        || tab.role_profile != *expected_profile
        || mux.find_workspace_for_tab(tab_id) != mux.find_workspace_for_tab(caller_tab_id);
    if invalid_hierarchy {
        mux.close_tab(tab_id)?;
        return Err("Team member spawn did not preserve the manager hierarchy".into());
    }
    let team = mux
        .get_team(team_id)
        .cloned()
        .ok_or_else(|| format!("Team {team_id} not found after member spawn"))?;
    crate::service::publish_mux_state_changed(events);
    Ok(AddTeamMemberResult {
        tab_id: tab_id.into(),
        team,
    })
}

fn team_member_role(requested: &str) -> Result<(String, Option<String>), String> {
    match requested.trim().to_ascii_lowercase().as_str() {
        "worker" => Ok(("Worker".into(), None)),
        "planner" => Ok(("Manager".into(), Some("planner".into()))),
        "orchestrator" => Ok(("Orchestrator".into(), None)),
        other => Err(format!(
            "Team member role must be worker, planner, or orchestrator, got '{other}'"
        )),
    }
}

pub(crate) fn rename_team_core(
    mux: &mut MultiplexerState,
    team_id: &str,
    name: String,
    caller_tab_id: Option<&str>,
) -> Result<Team, String> {
    authorize_team_manager(mux, team_id, caller_tab_id)?;
    mux.rename_team(team_id, name)
}

pub(crate) fn rename_team_with_state_change(
    mux: &mut MultiplexerState,
    events: &crate::api::events::EventBroadcaster,
    team_id: &str,
    name: String,
    caller_tab_id: Option<&str>,
) -> Result<Team, String> {
    let team = rename_team_core(mux, team_id, name, caller_tab_id)?;
    crate::service::publish_mux_state_changed(events);
    Ok(team)
}

pub(crate) fn close_team_with_state_change(
    mux: &mut MultiplexerState,
    events: &crate::api::events::EventBroadcaster,
    plan_store: &crate::service::plan::PlanStore,
    team_id: &str,
    caller_tab_id: Option<&str>,
    now_ms: u64,
) -> Result<(Team, CloseTeamCleanup, Vec<String>), String> {
    close_team_with_state_change_using(
        mux,
        events,
        plan_store,
        team_id,
        caller_tab_id,
        now_ms,
        |mux, tab_id| mux.close_tab_for_team_close(tab_id),
    )
}

fn close_team_with_state_change_using<F>(
    mux: &mut MultiplexerState,
    events: &crate::api::events::EventBroadcaster,
    plan_store: &crate::service::plan::PlanStore,
    team_id: &str,
    caller_tab_id: Option<&str>,
    now_ms: u64,
    mut close_member: F,
) -> Result<(Team, CloseTeamCleanup, Vec<String>), String>
where
    F: FnMut(&mut MultiplexerState, &str) -> Result<Vec<String>, String>,
{
    authorize_team_manager(mux, team_id, caller_tab_id)?;
    let member_tab_ids: Vec<String> = mux
        .snapshot()
        .tabs
        .values()
        .filter(|tab| tab.team_id.as_deref() == Some(team_id))
        .map(|tab| tab.id.clone())
        .collect();
    let chat_backings = member_tab_ids
        .iter()
        .filter_map(|tab_id| match mux.get_tab(tab_id)?.backing.clone() {
            TabBacking::ChatBackend {
                plugin_id,
                session_id,
            } => Some((plugin_id, session_id)),
            TabBacking::Pty => None,
        })
        .collect();
    let mut pty_ids = Vec::new();
    let mut closed_roots = Vec::new();
    let mut mux_transaction = mux.begin_team_close_transaction();
    for tab_id in &member_tab_ids {
        if mux_transaction.state_mut().get_tab(tab_id).is_some() {
            pty_ids.extend(close_member(mux_transaction.state_mut(), tab_id)?);
            closed_roots.push(tab_id.clone());
        }
    }
    let team = mux_transaction
        .state_mut()
        .remove_team(team_id)
        .ok_or_else(|| format!("Team {team_id} not found"))?;
    let plan_transaction = plan_store
        .archive_and_detach_team_plans_transaction(team_id, now_ms)
        .map_err(|error| error.to_string())?;
    let changed_plan_ids = plan_transaction.changed_plan_ids().to_vec();
    mux_transaction.commit(mux, &closed_roots);
    crate::service::publish_mux_state_changed(events);
    Ok((
        team,
        CloseTeamCleanup {
            member_tab_ids,
            chat_backings,
            pty_ids,
        },
        changed_plan_ids,
    ))
}

pub(crate) fn reconcile_dangling_team_plan_bindings(
    mux: &MultiplexerState,
    plan_store: &crate::service::plan::PlanStore,
    now_ms: u64,
) -> Result<Vec<String>, crate::service::plan::PlanError> {
    let live_team_ids = mux.list_teams().into_iter().map(|team| team.id).collect();
    plan_store.archive_active_plans_without_live_team(&live_team_ids, now_ms)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn authorize_team_manager(
    mux: &MultiplexerState,
    team_id: &str,
    caller_tab_id: Option<&str>,
) -> Result<(), String> {
    if mux.get_team(team_id).is_none() {
        return Err(format!("Team {team_id} not found"));
    }
    let Some(caller_tab_id) = caller_tab_id else {
        return Ok(());
    };
    let caller = mux
        .get_tab(caller_tab_id)
        .ok_or_else(|| format!("Caller tab {caller_tab_id} not found"))?;
    if caller.team_id.as_deref() != Some(team_id) {
        return Err(format!(
            "Caller tab {caller_tab_id} is not a member of Team {team_id}"
        ));
    }
    let manager = caller.role == Some(AgentRole::Orchestrator)
        || (caller.role == Some(AgentRole::Manager)
            && caller.role_profile.as_deref() == Some("planner"));
    if !manager {
        return Err(format!(
            "Caller tab {caller_tab_id} is not authorized to manage Team {team_id}"
        ));
    }
    Ok(())
}

fn authorize_team_mutation(
    mux: &MultiplexerState,
    team_id: &str,
    caller: TeamMutationCaller,
) -> Result<(), String> {
    match caller {
        TeamMutationCaller::TrustedDirectUser => authorize_team_manager(mux, team_id, None),
        TeamMutationCaller::AgentTab(tab_id) => authorize_team_manager(mux, team_id, Some(&tab_id)),
    }
}

#[cfg(test)]
mod tests;
