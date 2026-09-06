use codeterm_types::agent::role_kind::{self, builtins::ORCHESTRATOR, builtins::PLANNER};
use codeterm_types::team::Team;
use tauri::Manager;

use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{AgentRole, TabBacking};
use crate::service::plan::PlanError;
use crate::service::ServiceContext;

#[derive(Debug)]
pub(crate) enum TeamPlanError {
    Team(String),
    Plan(PlanError),
}

/// Who is asking to mutate a Team. `Human` is a named value rather than an
/// absent caller id, so "nobody supplied a tab" can never read as authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TeamMutationCaller {
    AgentTab(String),
    Human,
}

impl TeamMutationCaller {
    pub(crate) fn from_tab(caller_tab_id: Option<&str>) -> Self {
        match caller_tab_id.map(str::trim).filter(|id| !id.is_empty()) {
            Some(tab_id) => Self::AgentTab(tab_id.to_string()),
            None => Self::Human,
        }
    }

    fn tab_id(&self) -> Option<&str> {
        match self {
            Self::AgentTab(tab_id) => Some(tab_id),
            Self::Human => None,
        }
    }
}

impl ServiceContext {
    pub(crate) fn live_team_ids(&self) -> std::collections::BTreeSet<String> {
        self.mux
            .read()
            .list_teams()
            .into_iter()
            .map(|team| team.id)
            .collect()
    }

    pub(crate) fn reconcile_dangling_team_plan_bindings(
        &self,
        live_team_ids: &std::collections::BTreeSet<String>,
        now_ms: u64,
    ) -> Result<Vec<String>, crate::service::plan::PlanError> {
        let changed_plan_ids = self
            .plan_store
            .archive_active_plans_without_live_team(live_team_ids, now_ms)?;
        for plan_id in &changed_plan_ids {
            self.emit_plan_changed(plan_id);
        }
        Ok(changed_plan_ids)
    }

    pub(crate) async fn add_team_member(
        &self,
        team_id: &str,
        caller_tab_id: Option<&str>,
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
        let anchors = staffing::role_anchors_team(
            &prepared.expected_role,
            prepared.expected_profile.as_deref(),
        );
        let result = {
            let mut mux = self.mux.write();
            let mut result = commit_team_member_spawn(
                &mut mux,
                &self.events,
                team_id,
                caller_tab_id,
                &tab_id,
                &prepared.expected_role,
                &prepared.expected_profile,
            );
            // An anchor only ever lands in a Team with no live Orchestrator, so
            // the plan it finds there is by definition owned by a gone tab.
            if anchors {
                if let Ok(committed) = result.as_mut() {
                    committed.plan_takeover =
                        take_over_team_plan(&mut mux, &self.plan_store, team_id, &tab_id, now_ms());
                }
            }
            result
        };
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                // commit's invalid-hierarchy branch closed the member inside the
                // mux-level fn; persist here, the nearest point with &self.
                crate::service::session_persist::persist_session_after_close(self);
                if let Some(lane) = lane.take() {
                    let _ = tauri::async_runtime::spawn_blocking(move || lane.cleanup()).await;
                }
                return Err(error);
            }
        };
        if let Some(takeover) = result
            .plan_takeover
            .as_ref()
            .filter(|takeover| takeover.error.is_none())
        {
            self.emit_plan_changed(&takeover.plan_id);
        }
        self.emit_state_changed_after_bus_publish();
        Ok(result)
    }

    fn attach_manager_bootstrap(
        &self,
        team_id: &str,
        caller_tab_id: Option<&str>,
        params: &mut crate::service::SpawnAgentParams,
    ) -> Result<(), String> {
        let role_kind = match params.role_kind() {
            Some(kind @ (ORCHESTRATOR | PLANNER)) => kind.to_string(),
            _ => return Ok(()),
        };
        let role =
            role_kind::kind_label(Some(AgentRole::Manager), Some(&role_kind)).unwrap_or("Manager");
        let (team_name, project) = {
            let mux = self.mux.read();
            let team = mux
                .get_team(team_id)
                .ok_or_else(|| format!("Team {team_id} not found"))?;
            // A Team stranded without members still records its workspace, so a
            // recovery add can name the project it belongs to.
            let workspace_id = mux
                .team_workspace(team_id)
                .unwrap_or_else(|| team.workspace_id.clone());
            let project = mux
                .get_workspace(&workspace_id)
                .map(|workspace| workspace.name.clone())
                .unwrap_or(workspace_id);
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
        // A caller-less add is the bootstrap door and can only place an anchoring role, so there is no counterpart to name and nothing to introduce.
        let coordination = caller_tab_id
            .filter(|_| role_kind == PLANNER)
            .map(|caller_tab_id| {
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
            role_kind,
            team_id: team_id.into(),
            team_name,
            plan_id,
            project,
            coordination,
        });
        Ok(())
    }

    /// Attach an already-running roleless agent tab to a Team the caller
    /// already belongs to.
    pub(crate) fn adopt_agent_for_team(
        &self,
        team_id: &str,
        tab_id: &str,
        caller_tab_id: &str,
    ) -> Result<Team, String> {
        let team = {
            let mut mux = self.mux.write();
            reject_remote_team_mutation(&mux, team_id)?;
            let team = adopt_agent_into_team(&mut mux, team_id, tab_id, caller_tab_id)?;
            crate::service::publish_mux_state_changed(&self.events);
            team
        };
        self.emit_state_changed_after_bus_publish();
        Ok(team)
    }

    pub(crate) fn rename_team(
        &self,
        team_id: &str,
        name: String,
        caller: TeamMutationCaller,
    ) -> Result<Team, String> {
        let mut mux = self.mux.write();
        let team = rename_team_with_state_change(&mut mux, &self.events, team_id, name, &caller)?;
        drop(mux);
        self.emit_state_changed_after_bus_publish();
        Ok(team)
    }

    pub(crate) fn close_team(
        &self,
        team_id: &str,
        caller: TeamMutationCaller,
    ) -> Result<Team, String> {
        let (team, cleanup, changed_plan_ids) = {
            let mut mux = self.mux.write();
            close_team_with_state_change(
                &mut mux,
                &self.events,
                &self.plan_store,
                team_id,
                &caller,
                now_ms(),
            )?
        };
        // Persist the authorized reduction before any fallible cleanup can
        // early-return and leave last.json still holding the member tabs.
        crate::service::session_persist::persist_session_after_close(self);
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
        for tab_id in &cleanup.member_tab_ids {
            self.revoke_agent_pad(tab_id)?;
        }
        self.delete_team_crons(team_id);
        self.emit_state_changed_after_bus_publish();
        Ok(team)
    }

    fn delete_team_crons(&self, team_id: &str) {
        let Some(cron) = self.app_handle.try_state::<crate::CronState>() else {
            return;
        };
        let Some(wake) = self.app_handle.try_state::<crate::CronWake>() else {
            return;
        };
        let job_ids: Vec<String> = cron
            .lock()
            .list_for_team(team_id)
            .into_iter()
            .map(|job| job.id.clone())
            .collect();
        for job_id in job_ids {
            if let Err(error) = self.cron_delete(cron.inner(), wake.inner(), &job_id) {
                log::error!("Failed to delete Team cron {job_id}: {error}");
            }
        }
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
    authorize_team_mutation(mux, team_id, &caller).map_err(|error| {
        TeamPlanError::Team(format!(
            "Plan adoption requires manager authority in destination Team {team_id}: {error}. \
Safe recovery: old-team member transfers ownership, then new owner adopts."
        ))
    })?;
    let owner_tab_id = team_plan_owner(mux, plan_store, team_id, plan_id, &caller)?;
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
    let source_denial = match authorize_team_mutation(mux, source_team_id, &caller) {
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
manager or destination Orchestrator {destination_owner_tab_id} in Team \
{destination_team_id} must authorize the transfer. Safe recovery: old-team member \
transfers ownership, then new owner adopts."
        )),
        // A human has no destination-owner identity to fall back on, so the
        // source refusal (missing or remote-owned Team) stands verbatim.
        TeamMutationCaller::Human => Err(source_denial),
    }
}

/// Resolution is local to the caller's own placement, so several anchors stay
/// unambiguous without a global "there must be exactly one" scan.
fn anchor_above(mux: &MultiplexerState, team_id: &str, caller_tab_id: &str) -> Option<String> {
    let mut seen = std::collections::HashSet::new();
    let mut current = caller_tab_id.to_string();
    while seen.insert(current.clone()) {
        let tab = mux.get_tab(&current)?;
        if tab.team_id.as_deref() == Some(team_id) && tab.anchors_team() {
            return Some(current);
        }
        current = tab.parent_tab_id.clone()?;
    }
    None
}

/// An agent caller resolves through its own chain; a human caller keeps the
/// plan's existing owner, and is otherwise asked to name one.
fn team_plan_owner(
    mux: &MultiplexerState,
    plan_store: &crate::service::plan::PlanStore,
    team_id: &str,
    plan_id: &str,
    caller: &TeamMutationCaller,
) -> Result<String, TeamPlanError> {
    let anchors = local_team_anchors(mux, team_id);
    if anchors.is_empty() {
        return Err(TeamPlanError::Team(format!(
            "Team {team_id} has no live Orchestrator"
        )));
    }
    if let Some(caller_tab_id) = caller.tab_id() {
        return anchor_above(mux, team_id, caller_tab_id).ok_or_else(|| {
            TeamPlanError::Team(format!(
                "Caller tab {caller_tab_id} sits under no anchor in Team {team_id}"
            ))
        });
    }
    let current_owner = plan_store
        .get(plan_id)
        .and_then(|plan| plan.owner.tab_id)
        .filter(|tab_id| anchors.iter().any(|anchor| anchor == tab_id));
    match (current_owner, anchors.as_slice()) {
        (Some(owner), _) => Ok(owner),
        (None, [only]) => Ok(only.clone()),
        (None, many) => Err(TeamPlanError::Team(format!(
            "Team {team_id} has several anchors ({}); adopt from one of them so the owner is explicit",
            many.join(", ")
        ))),
    }
}

#[derive(Debug)]
pub(crate) struct CloseTeamCleanup {
    member_tab_ids: Vec<String>,
    chat_backings: Vec<(String, String)>,
    pty_ids: Vec<String>,
}

pub(crate) fn rename_team_core(
    mux: &mut MultiplexerState,
    team_id: &str,
    name: String,
    caller: &TeamMutationCaller,
) -> Result<Team, String> {
    authorize_team_manager(mux, team_id, caller)?;
    mux.rename_team(team_id, name)
}

pub(crate) fn rename_team_with_state_change(
    mux: &mut MultiplexerState,
    events: &crate::api::events::EventBroadcaster,
    team_id: &str,
    name: String,
    caller: &TeamMutationCaller,
) -> Result<Team, String> {
    let team = rename_team_core(mux, team_id, name, caller)?;
    crate::service::publish_mux_state_changed(events);
    Ok(team)
}

pub(crate) fn close_team_with_state_change(
    mux: &mut MultiplexerState,
    events: &crate::api::events::EventBroadcaster,
    plan_store: &crate::service::plan::PlanStore,
    team_id: &str,
    caller: &TeamMutationCaller,
    now_ms: u64,
) -> Result<(Team, CloseTeamCleanup, Vec<String>), String> {
    close_team_with_state_change_using(
        mux,
        events,
        plan_store,
        team_id,
        caller,
        now_ms,
        |mux, tab_id| mux.close_tab_for_team_close(tab_id),
    )
}

fn close_team_with_state_change_using<F>(
    mux: &mut MultiplexerState,
    events: &crate::api::events::EventBroadcaster,
    plan_store: &crate::service::plan::PlanStore,
    team_id: &str,
    caller: &TeamMutationCaller,
    now_ms: u64,
    mut close_member: F,
) -> Result<(Team, CloseTeamCleanup, Vec<String>), String>
where
    F: FnMut(&mut MultiplexerState, &str) -> Result<Vec<String>, String>,
{
    let member_tab_ids: Vec<String> = mux
        .snapshot()
        .tabs
        .values()
        .filter(|tab| tab.team_id.as_deref() == Some(team_id))
        .map(|tab| tab.id.clone())
        .collect();
    authorize_team_close(mux, team_id, caller, &member_tab_ids)?;
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

#[cfg(test)]
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

/// Local tabs that anchor this Team. A mirrored (remote) Team has members here
/// but no anchor, which is what makes it locally immutable.
fn local_team_anchors(mux: &MultiplexerState, team_id: &str) -> Vec<String> {
    let mut anchors: Vec<String> = mux
        .snapshot()
        .tabs
        .values()
        .filter(|tab| tab.team_id.as_deref() == Some(team_id) && tab.anchors_team())
        .map(|tab| tab.id.clone())
        .collect();
    anchors.sort();
    anchors
}

/// The peer a Team is anchored on, read from existing `RemoteOwnership`
/// evidence. `RemoteOwnership` itself is untouched: this only reads it.
fn remote_team_origin(mux: &MultiplexerState, team_id: &str) -> Option<String> {
    mux.snapshot().tabs.values().find_map(|tab| {
        let ownership = tab.ownership.as_ref()?;
        let team = ownership.team.as_ref()?;
        (team.id == team_id).then(|| ownership.peer_name.clone())
    })
}

/// A Team whose anchor lives on another peer is mirrored here for visibility
/// only; mutating it locally would fork it from the authority that owns it.
fn reject_remote_team_mutation(mux: &MultiplexerState, team_id: &str) -> Result<(), String> {
    if !local_team_anchors(mux, team_id).is_empty() {
        return Ok(());
    }
    match remote_team_origin(mux, team_id) {
        Some(peer) => Err(format!(
            "Team {team_id} is anchored on peer {peer}; it is read-only on this machine. \
Safe recovery: run the mutation on {peer}."
        )),
        None => Ok(()),
    }
}

pub(super) fn authorize_team_manager(
    mux: &MultiplexerState,
    team_id: &str,
    caller: &TeamMutationCaller,
) -> Result<(), String> {
    if mux.get_team(team_id).is_none() {
        return Err(format!("Team {team_id} not found"));
    }
    reject_remote_team_mutation(mux, team_id)?;
    let Some(caller_tab_id) = caller.tab_id() else {
        return Ok(());
    };
    let caller_tab = mux
        .get_tab(caller_tab_id)
        .ok_or_else(|| format!("Caller tab {caller_tab_id} not found"))?;
    if caller_tab.team_id.as_deref() != Some(team_id) {
        return Err(format!(
            "Caller tab {caller_tab_id} is not a member of Team {team_id}"
        ));
    }
    if !caller_tab.has_team_authority() {
        return Err(format!(
            "Caller tab {caller_tab_id} is not authorized to manage Team {team_id}"
        ));
    }
    Ok(())
}

/// Close is the one Team mutation a zero-member Team must still be able to
/// satisfy: the ordinary member-authority check below requires the caller to
/// currently BE a member, which a Team with no members can never again make
/// true. Closing an empty Team is the normal end of its life, not a mutation
/// that needs an existing member's authority — there is nothing left to
/// protect. Every other mutation (rename, add member, adopt a plan) keeps the
/// stricter `authorize_team_manager` gate.
fn authorize_team_close(
    mux: &MultiplexerState,
    team_id: &str,
    caller: &TeamMutationCaller,
    member_tab_ids: &[String],
) -> Result<(), String> {
    if mux.get_team(team_id).is_none() {
        return Err(format!("Team {team_id} not found"));
    }
    reject_remote_team_mutation(mux, team_id)?;
    if member_tab_ids.is_empty() {
        return Ok(());
    }
    authorize_team_manager(mux, team_id, caller)
}

fn authorize_team_mutation(
    mux: &MultiplexerState,
    team_id: &str,
    caller: &TeamMutationCaller,
) -> Result<(), String> {
    authorize_team_manager(mux, team_id, caller)
}

mod membership;
pub(crate) use membership::{
    adopt_agent_into_team, apply_team_attachment, resolve_inherited_team_attachment,
    resolve_spawn_attachment,
};

mod staffing;
pub(crate) use staffing::{
    commit_team_member_spawn, prepare_team_member_spawn_with_provider_validation,
    take_over_team_plan, AddTeamMemberParams, AddTeamMemberResult,
};

#[cfg(test)]
mod general_agent_guard_tests;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod transfer_tests;

#[cfg(test)]
mod workspace_tests;
