use codeterm_types::agent::role_kind::{self, builtins::ORCHESTRATOR, builtins::PLANNER};
use codeterm_types::team::Team;
use tauri::Manager;

use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{AgentRole, TabCloseReason};
use crate::service::plan::PlanError;
use crate::service::ServiceContext;

mod close;
#[cfg(test)]
pub(crate) use close::close_team_with_state_change;
#[cfg(test)]
use close::close_team_with_state_change_using;
use close::{
    commit_prepared_team_close, finish_team_lane_cleanup, prepare_team_close, team_close_lanes,
};

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
        crate::service::spawn::validate_agent_model(
            &self.agent_registry,
            &params.provider,
            params.model.as_deref(),
        )
        .map_err(|error| error.to_string())?;
        let is_cartographer = params.role.trim().eq_ignore_ascii_case("cartographer");
        let _cartographer_guard = if is_cartographer {
            Some(
                self.cartographer_attach_locks
                    .for_team(team_id)
                    .lock_owned()
                    .await,
            )
        } else {
            None
        };
        let prepared = {
            let mux = self.mux.read();
            let prepared = prepare_team_member_spawn(&mux, team_id, caller_tab_id, params)?;
            if is_cartographer {
                if let Some(existing) = staffing::existing_cartographer_attachment(&mux, team_id) {
                    return Ok(existing);
                }
            }
            self.classify_spawn_target_for_params(
                prepared.spawn_params.agent.as_deref().unwrap_or_default(),
                &prepared.spawn_params,
            )?;
            prepared
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
        let (tab_id, spawn_warning) = match spawn {
            Ok(spawn) => (spawn.tab_id, spawn.warning),
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
            commit_team_member_with_plan_takeover(
                &mut mux,
                &self.events,
                team_id,
                caller_tab_id,
                &tab_id,
                &prepared.expected_role,
                &prepared.expected_profile,
                anchors,
                |mux| take_over_team_plan(mux, &self.plan_store, team_id, &tab_id, now_ms()),
            )
        };
        let mut result = match result {
            Ok(result) => result,
            Err(error) => {
                // Commit and takeover failures close the spawned member inside
                // the mux-level transaction; persist at the nearest &self.
                crate::service::session_persist::persist_session_after_close(self);
                if let Some(lane) = lane.take() {
                    let _ = tauri::async_runtime::spawn_blocking(move || lane.cleanup()).await;
                }
                return Err(error);
            }
        };
        result.warning = spawn_warning;
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

    pub(crate) async fn close_team(
        &self,
        team_id: &str,
        caller: TeamMutationCaller,
    ) -> Result<Team, String> {
        let close_at = now_ms();
        let prepared = {
            let mux = self.mux.read();
            prepare_team_close(&mux, team_id, &caller)?
        };
        let lane_plan_store = std::sync::Arc::clone(&self.plan_store);
        let lane_team_id = team_id.to_string();
        let lane_member_ids = prepared.member_tab_ids.clone();
        let lanes = tokio::task::spawn_blocking(move || {
            let plans = lane_plan_store.list_full();
            team_close_lanes(&plans, &lane_team_id, &lane_member_ids)
        })
        .await
        .map_err(|error| format!("Team lane inventory task failed: {error}"))?;
        let plan_store = std::sync::Arc::clone(&self.plan_store);
        let transaction_team_id = team_id.to_string();
        let transaction_member_ids = prepared.member_tab_ids.clone();
        let plan_transaction = tokio::task::spawn_blocking(move || {
            plan_store.close_team_task_attempts_transaction(
                &transaction_team_id,
                &transaction_member_ids,
                close_at,
            )
        })
        .await
        .map_err(|error| format!("Team plan close task failed: {error}"))?
        .map_err(|error| error.to_string())?;
        let changed_plan_ids = plan_transaction.changed_plan_ids().to_vec();
        let commit = {
            let mut mux = self.mux.write();
            commit_prepared_team_close(&mut mux, team_id, &caller, &prepared)
        };
        let (team, cleanup) = match commit {
            Ok(closed) => closed,
            Err(error) => {
                let rollback_store = std::sync::Arc::clone(&self.plan_store);
                tokio::task::spawn_blocking(move || {
                    rollback_store.rollback_team_plan_archive(plan_transaction)
                })
                .await
                .map_err(|rollback| format!("{error}; plan rollback task failed: {rollback}"))?
                .map_err(|rollback| format!("{error}; plan rollback failed: {rollback}"))?;
                return Err(error);
            }
        };
        crate::service::publish_mux_state_changed(&self.events);
        // Persist the authorized reduction before any fallible cleanup can
        // early-return and leave last.json still holding the member tabs.
        crate::service::session_persist::persist_session_after_close(self);
        let registry = std::sync::Arc::clone(&self.agent_registry);
        let closed_tabs = cleanup.agent_providers;
        if let Err(error) = tokio::task::spawn_blocking(move || {
            crate::service::launch_artifacts::release_closed_tabs(&registry, &closed_tabs)
        })
        .await
        {
            log::warn!("[team-close] launch artifact release task failed: {error}");
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
        let plan_store = std::sync::Arc::clone(&self.plan_store);
        let lane_cleanup = tokio::task::spawn_blocking(move || {
            finish_team_lane_cleanup(&plan_store, &lanes, close_at)
        })
        .await
        .map_err(|error| format!("Team lane cleanup task failed: {error}"))
        .and_then(|result| result);
        if let Err(error) = &lane_cleanup {
            log::error!("{error}");
        }
        for plan_id in changed_plan_ids {
            self.emit_plan_changed(&plan_id);
        }
        for tab_id in &cleanup.member_tab_ids {
            self.revoke_agent_pad(tab_id)?;
        }
        self.delete_team_crons(team_id);
        self.emit_state_changed_after_bus_publish();
        lane_cleanup?;
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
        self.resume_plan_operation(plan_id, now_ms)
            .map(|outcome| outcome.mode)
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

    pub(super) fn valid_live_plan_team(
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

#[allow(clippy::too_many_arguments)]
fn commit_team_member_with_plan_takeover<F>(
    mux: &mut MultiplexerState,
    events: &crate::api::events::EventBroadcaster,
    team_id: &str,
    caller_tab_id: Option<&str>,
    tab_id: &str,
    expected_role: &str,
    expected_profile: &Option<String>,
    anchors: bool,
    take_over: F,
) -> Result<AddTeamMemberResult, String>
where
    F: FnOnce(&mut MultiplexerState) -> Option<staffing::PlanTakeover>,
{
    let mut result = commit_team_member_spawn(
        mux,
        events,
        team_id,
        caller_tab_id,
        tab_id,
        expected_role,
        expected_profile,
    )?;
    if !anchors {
        return Ok(result);
    }
    let takeover = take_over(mux);
    if let Some(cause) = takeover
        .as_ref()
        .and_then(|takeover| takeover.error.as_deref())
    {
        if let Some(tab) = mux.get_tab_mut(tab_id) {
            tab.team_id = None;
        }
        mux.close_tab_with_reason(tab_id, TabCloseReason::SpawnRollback)
            .map_err(|rollback| {
                format!("plan takeover failed: {cause}; rollback failed: {rollback}")
            })?;
        return Err(format!("plan takeover failed: {cause}"));
    }
    result.plan_takeover = takeover;
    Ok(result)
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
pub(super) fn authorize_team_close(
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
#[cfg(test)]
pub(crate) use staffing::prepare_team_member_spawn_with_provider_validation;
pub(crate) use staffing::{
    commit_team_member_spawn, prepare_team_member_spawn, take_over_team_plan, AddTeamMemberParams,
    AddTeamMemberResult, TeamCartographerAttachLocks,
};

#[cfg(test)]
mod general_agent_guard_tests;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod transfer_tests;

#[cfg(test)]
mod workspace_tests;
