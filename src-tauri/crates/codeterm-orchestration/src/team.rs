use codeterm_types::agent::role_kind::{self, builtins::ORCHESTRATOR, builtins::PLANNER};
use codeterm_types::team::Team;
use tauri::Manager;

use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::multiplexer::types::{AgentRole, TabCloseReason};
use codeterm_service_core::service::plan::PlanError;
use codeterm_service_core::service::team_core::{
    is_remote_team, local_team_anchors, now_ms, remote_team_origin, valid_live_plan_team,
    TeamMutationCaller, TeamPlanError,
};
use codeterm_service_core::service::ServiceContext;

pub mod close;
mod close_busy;
mod storage_teardown;
#[cfg(any(test, feature = "test-support"))]
pub use close::close_team_with_state_change;
#[cfg(test)]
use close::close_team_with_state_change_using;
pub use close::TeamCloseResult;
use close::{
    commit_prepared_team_close, finish_team_lane_cleanup, prepare_team_close, team_close_lanes,
};

pub trait TeamExt {
    fn rename_team(
        &self,
        team_id: &str,
        name: String,
        caller: TeamMutationCaller,
    ) -> Result<Team, String>;
    fn close_team(
        &self,
        team_id: &str,
        caller: TeamMutationCaller,
        force: bool,
    ) -> impl std::future::Future<Output = Result<TeamCloseResult, String>> + Send;
}

impl TeamExt for ServiceContext {
    fn rename_team(
        &self,
        team_id: &str,
        name: String,
        caller: TeamMutationCaller,
    ) -> Result<Team, String> {
        let mut mux = self.mux.write();
        let team = rename_team_with_state_change(&mut mux, &self.events, team_id, name, &caller)?;
        drop(mux);
        self.emit_state_changed_after_bus_publish();
        let _ = codeterm_service_core::service::session_persist::persist_session_after_mutation(
            self,
            codeterm_service_core::service::session_persist::SessionPersistUrgency::Prompt,
        );
        Ok(team)
    }

    /// Refuses while a Team task is in progress or a Worker is working unless
    /// `force`; a forced close reports every killed Worker and task.
    async fn close_team(
        &self,
        team_id: &str,
        caller: TeamMutationCaller,
        force: bool,
    ) -> Result<TeamCloseResult, String> {
        let force = force
            || matches!(
                caller,
                TeamMutationCaller::Human | TeamMutationCaller::HumanConfirmedHeaderless
            );
        let caller_kind = caller.kind();
        let workspace = self
            .mux
            .read()
            .get_team(team_id)
            .ok_or_else(|| format!("Team {team_id} not found"))?
            .workspace_id
            .clone();
        let service = self.clone();
        let capture_team = team_id.to_string();
        let inventory = tokio::task::spawn_blocking(move || {
            service.capture_close_inventory(&workspace, Some(&capture_team))
        })
        .await
        .map_err(|error| error.to_string())??;
        let result = close_team_inner(self, team_id, caller, force).await;
        let service = self.clone();
        match tokio::task::spawn_blocking(move || {
            service.settle_close_inventory(inventory.as_deref())
        })
        .await
        {
            Ok(Ok(())) => {}
            other => log::warn!("[team-close] close history settlement failed: {other:?}"),
        }
        if let Err(error) = &result {
            log::warn!("[team-close] caller={caller_kind} team={team_id} failed: {error}");
        }
        result
    }
}

pub async fn add_team_member(
    svc: &ServiceContext,
    team_id: &str,
    caller_tab_id: Option<&str>,
    params: AddTeamMemberParams,
) -> Result<AddTeamMemberResult, String> {
    codeterm_service_core::service::spawn::validate_agent_model(
        &svc.agent_registry,
        &params.provider,
        params.model.as_deref(),
    )
    .map_err(|error| error.to_string())?;
    let is_cartographer = params.role.trim().eq_ignore_ascii_case("cartographer");
    let is_orchestrator = params.role.trim().eq_ignore_ascii_case("orchestrator");
    let _cartographer_guard = if is_cartographer {
        Some(
            svc.ext::<crate::CartographerAttachLocks>()
                .0
                .for_team(team_id)
                .lock_owned()
                .await,
        )
    } else {
        None
    };
    let _orchestrator_guard = if is_orchestrator {
        Some(
            svc.ext::<crate::OrchestratorAttachLocks>()
                .0
                .for_team(team_id)
                .lock_owned()
                .await,
        )
    } else {
        None
    };
    let (classified, existing) = {
        let mux = svc.mux.read();
        let prepared = prepare_team_member_spawn(&mux, team_id, caller_tab_id, params.clone())?;
        let existing = is_cartographer
            .then(|| staffing::existing_cartographer_attachment(&mux, team_id))
            .flatten();
        (prepared, existing)
    };
    if let Some(existing) = existing {
        return staffing::reuse_cartographer_with_brief(svc, existing, params.task).await;
    }
    svc.classify_spawn_target_for_params(
        classified.spawn_params.agent.as_deref().unwrap_or_default(),
        &classified.spawn_params,
    )?;
    let brief = params.task.clone();
    let (prepared, detached_failed_anchor, existing) = {
        let mut mux = svc.mux.write();
        let prepared = prepare_team_member_spawn(&mux, team_id, caller_tab_id, params)?;
        let existing = is_cartographer
            .then(|| staffing::existing_cartographer_attachment(&mux, team_id))
            .flatten();
        let anchors = staffing::role_anchors_team(
            &prepared.expected_role,
            prepared.expected_profile.as_deref(),
        );
        let detached =
            anchors && staffing::detach_spawn_failed_team_orchestrators(&mut mux, team_id);
        (prepared, detached, existing)
    };
    if let Some(existing) = existing {
        return staffing::reuse_cartographer_with_brief(svc, existing, brief).await;
    }
    if detached_failed_anchor {
        codeterm_service_core::service::publish_mux_state_changed(&svc.events);
        codeterm_service_core::service::session_persist::persist_session_after_close(svc);
        svc.emit_state_changed_after_bus_publish();
    }
    let mut spawn_params = prepared.spawn_params;
    attach_manager_bootstrap(svc, team_id, caller_tab_id, &mut spawn_params)?;
    let lane_label = format!("team-{team_id}");
    let service = svc.clone();
    let (spawn_params, mut lane) = tauri::async_runtime::spawn_blocking(move || {
        let lane = service.provision_worker_lane_for_spawn(&mut spawn_params, &lane_label)?;
        Ok::<_, String>((spawn_params, lane))
    })
    .await
    .map_err(|error| format!("worker lane provisioning failed: {error}"))??;
    let spawn = svc
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
        let mut mux = svc.mux.write();
        commit_team_member_with_plan_takeover(
            &mut mux,
            &svc.events,
            team_id,
            caller_tab_id,
            &tab_id,
            &prepared.expected_role,
            &prepared.expected_profile,
            anchors,
            |mux| take_over_team_plan(mux, &svc.plan_store, team_id, &tab_id, now_ms()),
        )
    };
    let mut result = match result {
        Ok(result) => result,
        Err(error) => {
            // Commit and takeover failures close the spawned member inside
            // the mux-level transaction; persist at the nearest &self.
            codeterm_service_core::service::session_persist::persist_session_after_close(svc);
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
        svc.emit_plan_changed(&takeover.plan_id);
    }
    svc.emit_state_changed_after_bus_publish();
    Ok(result)
}

fn attach_manager_bootstrap(
    svc: &ServiceContext,
    team_id: &str,
    caller_tab_id: Option<&str>,
    params: &mut codeterm_service_core::service::SpawnAgentParams,
) -> Result<(), String> {
    let role_kind = match params.role_kind() {
        Some(kind @ (ORCHESTRATOR | PLANNER)) => kind.to_string(),
        _ => return Ok(()),
    };
    let role =
        role_kind::kind_label(Some(AgentRole::Manager), Some(&role_kind)).unwrap_or("Manager");
    let (team_name, project) = {
        let mux = svc.mux.read();
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
    let plan_id = svc
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
            let mux = svc.mux.read();
            let caller_label = mux.display_label_for_tab(caller_tab_id);
            let planner =
                codeterm_service_core::service::spawn_factory::Counterpart::new("Planner", "", "");
            let orchestrator = codeterm_service_core::service::spawn_factory::Counterpart::new(
                "Orchestrator",
                caller_label,
                caller_tab_id,
            );
            codeterm_service_core::service::spawn_factory::introduction_body(
                &planner,
                &orchestrator,
            )
        });
    params.manager_bootstrap = Some(
        codeterm_service_core::service::role_identity::ManagerBootstrapInputs {
            role: role.into(),
            role_kind,
            team_id: team_id.into(),
            team_name,
            plan_id,
            project,
            coordination,
        },
    );
    Ok(())
}

/// Attach an already-running roleless agent tab to a Team the caller
/// already belongs to.
pub fn adopt_agent_for_team(
    svc: &ServiceContext,
    team_id: &str,
    tab_id: &str,
    caller_tab_id: &str,
) -> Result<Team, String> {
    let team = {
        let mut mux = svc.mux.write();
        reject_remote_team_mutation(&mux, team_id)?;
        let team = adopt_agent_into_team(&mut mux, team_id, tab_id, caller_tab_id)?;
        codeterm_service_core::service::publish_mux_state_changed(&svc.events);
        team
    };
    svc.emit_state_changed_after_bus_publish();
    Ok(team)
}

async fn close_team_inner(
    svc: &ServiceContext,
    team_id: &str,
    caller: TeamMutationCaller,
    force: bool,
) -> Result<TeamCloseResult, String> {
    let close_at = now_ms();
    let (prepared, storage_teardown) =
        storage_teardown::prepare_storage_team_teardown(svc, team_id, &caller).await?;
    let lane_plan_store = std::sync::Arc::clone(&svc.plan_store);
    let plans = tokio::task::spawn_blocking(move || lane_plan_store.list_full())
        .await
        .map_err(|error| format!("Team lane inventory task failed: {error}"))?;
    let lanes = team_close_lanes(&plans, team_id, &prepared.member_tab_ids);
    let busy = {
        let mux = svc.mux.read();
        close_busy::team_close_busy(&mux, &plans, team_id, &prepared.member_tab_ids)
    };
    if !busy.is_empty() && !force {
        return Err(busy.refusal(team_id));
    }
    let plan_store = std::sync::Arc::clone(&svc.plan_store);
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
    let storage_commit = if let Some(teardown) = &storage_teardown {
        let completion = teardown.storage.team_teardown(teardown.request.clone());
        #[cfg(test)]
        if let Some(probe) = svc
            .app_handle
            .try_state::<storage_teardown::StorageTeardownSubmitted>()
        {
            let _ = probe.0.send(());
        }
        match completion.await {
            Ok(committed) => Some(committed),
            Err(error) => {
                let rollback_store = std::sync::Arc::clone(&svc.plan_store);
                let rollback = tokio::task::spawn_blocking(move || {
                    rollback_store.rollback_team_plan_archive(plan_transaction)
                })
                .await;
                let impact = match rollback {
                    Ok(Ok(())) => error.to_string(),
                    Ok(Err(rollback)) => {
                        format!("{error}; plan rollback failed: {rollback}")
                    }
                    Err(rollback) => {
                        format!("{error}; plan rollback task failed: {rollback}")
                    }
                };
                svc.notify_event(
                    "coordination-incident",
                    serde_json::json!({
                        "kind": "team_teardown_storage_commit_failed",
                        "impact": impact,
                        "fallback": "The Team and desk, cron and Sentinel rows remain active; retry after storage recovers."
                    }),
                );
                return Err(format!("Team storage teardown failed: {impact}"));
            }
        }
    } else {
        None
    };
    if let (Some(teardown), Some(committed)) = (&storage_teardown, &storage_commit) {
        {
            teardown.cron.lock().publish_external(
                &teardown.cron_changes,
                &committed.value.cron,
                committed.receipt.revision,
            );
        }
        let sentinel_commit = codeterm_service_core::storage::Committed {
            value: committed.value.sentinel.clone(),
            receipt: committed.receipt,
        };
        teardown
            .sentinel
            .store
            .adopt_external_commit(&sentinel_commit)?;
    }
    let commit = {
        let mut mux = svc.mux.write();
        if storage_commit.is_some() {
            let current = prepare_team_close(&mux, team_id, &caller)?;
            commit_prepared_team_close(&mut mux, team_id, &caller, &current)
        } else {
            commit_prepared_team_close(&mut mux, team_id, &caller, &prepared)
        }
    };
    let (team, cleanup) = match commit {
        Ok(closed) => closed,
        Err(error) => {
            let rollback_store = std::sync::Arc::clone(&svc.plan_store);
            tokio::task::spawn_blocking(move || {
                rollback_store.rollback_team_plan_archive(plan_transaction)
            })
            .await
            .map_err(|rollback| format!("{error}; plan rollback task failed: {rollback}"))?
            .map_err(|rollback| format!("{error}; plan rollback failed: {rollback}"))?;
            return Err(error);
        }
    };
    if storage_commit.is_none() {
        codeterm_service_core::service::publish_mux_state_changed(&svc.events);
    }
    // Persist the authorized reduction before any fallible cleanup can
    // early-return and leave last.json still holding the member tabs.
    codeterm_service_core::service::session_persist::persist_session_after_close(svc);
    for tab_id in &cleanup.member_tab_ids {
        svc.chat_registry.shutdown_tab(tab_id);
    }
    for (plugin_id, session_id) in &cleanup.chat_backings {
        let _ = svc.chatbackend_close(plugin_id, session_id);
    }
    for tab_id in &cleanup.member_tab_ids {
        svc.flag_bound_tasks_for_gone_tab(tab_id, 0);
        codeterm_service_core::service::prompt_resolution::clear_tab_prompt(None);
        codeterm_service_core::service::launch_readiness::owner_prompt_cleared(tab_id);
        svc.prompt_detector.lock().remove_tab(tab_id);
        codeterm_service_core::service::structured_prompt::clear_tab(tab_id);
        svc.remote_workers.forget_orchestrator(tab_id);
    }
    svc.kill_ptys(&cleanup.pty_ids);
    svc.dispose_closed_surface_runtimes();
    let registry = std::sync::Arc::clone(&svc.agent_registry);
    let closed_tabs = cleanup.agent_providers;
    if let Err(error) = tokio::task::spawn_blocking(move || {
        codeterm_service_core::service::launch_artifacts::release_closed_tabs(
            &registry,
            &closed_tabs,
        )
    })
    .await
    {
        log::warn!("[team-close] launch artifact release task failed: {error}");
    }
    let plan_store = std::sync::Arc::clone(&svc.plan_store);
    let lane_cleanup = tokio::task::spawn_blocking(move || {
        finish_team_lane_cleanup(&plan_store, &lanes, close_at)
    })
    .await
    .map_err(|error| format!("Team lane cleanup task failed: {error}"))
    .and_then(|result| result);
    if let Err(error) = &lane_cleanup {
        log::error!("{error}");
    }
    let flagged_plans = changed_plan_ids
        .iter()
        .filter_map(|plan_id| svc.plan_store.get(plan_id))
        .filter_map(|plan| {
            let reason = plan.orphaned.map(|orphan| orphan.reason)?;
            Some(close_busy::TeamCloseFlaggedPlan {
                plan_id: plan.id,
                reason,
            })
        })
        .collect();
    for plan_id in changed_plan_ids {
        svc.emit_plan_changed(&plan_id);
    }
    for tab_id in &cleanup.member_tab_ids {
        if let Err(error) = svc.revoke_agent_pad(tab_id) {
            log::warn!("[team-close] pad cleanup for {tab_id} failed: {error}");
        }
    }
    if let Some(teardown) = &storage_teardown {
        storage_teardown::finish_storage_team_teardown(svc, team_id, teardown).await;
        codeterm_service_core::service::publish_mux_state_changed(&svc.events);
    } else {
        finish_team_teardown(svc, team_id).await?;
    }
    let inventory = lane_cleanup?;
    if !busy.is_empty() {
        log::warn!(
            "[team-close] forced close of Team {team_id} killed {} Worker(s) and left {} in-progress task(s)",
            busy.workers.len(),
            busy.tasks.len()
        );
    }
    Ok(TeamCloseResult {
        team,
        inventory,
        killed_workers: busy.workers,
        unfinished_tasks: busy.tasks,
        flagged_plans,
    })
}

fn delete_team_crons(svc: &ServiceContext, team_id: &str) {
    let Some(cron) = svc
        .app_handle
        .try_state::<codeterm_service_core::CronState>()
    else {
        return;
    };
    let Some(wake) = svc
        .app_handle
        .try_state::<codeterm_service_core::CronWake>()
    else {
        return;
    };
    let job_ids: Vec<String> = cron
        .lock()
        .list_for_team(team_id)
        .into_iter()
        .map(|job| job.id.clone())
        .collect();
    for job_id in job_ids {
        if let Err(error) = svc.cron_delete(cron.inner(), wake.inner(), &job_id) {
            log::error!("Failed to delete Team cron {job_id}: {error}");
        }
    }
}

async fn finish_team_teardown(svc: &ServiceContext, team_id: &str) -> Result<(), String> {
    let service = svc.clone();
    let team_id = team_id.to_string();
    tokio::task::spawn_blocking(move || {
        delete_team_crons(&service, &team_id);
        if let Some(ports) = service
            .app_handle
            .try_state::<codeterm_service_core::ports::sentinel::SentinelPorts>()
        {
            ports.cleanup.remove_team(&team_id)?;
        }
        if let Err(error) =
            codeterm_service_core::delayed_send::delete_team_series(&service, &team_id)
        {
            log::error!("Failed to delete Team delayed series {team_id}: {error}");
        }
        service.emit_state_changed_after_bus_publish();
        Ok(())
    })
    .await
    .map_err(|error| format!("Team cron and Sentinel teardown task failed: {error}"))?
}

pub fn adopt_plan_for_team(
    svc: &ServiceContext,
    team_id: &str,
    plan_id: &str,
    caller: TeamMutationCaller,
    now_ms: u64,
) -> Result<codeterm_service_core::plan::Plan, TeamPlanError> {
    let mut mux = svc.mux.write();
    let plan =
        adopt_plan_for_team_core(&mut mux, &svc.plan_store, team_id, plan_id, caller, now_ms)?;
    drop(mux);
    codeterm_host_runtime::events::emit_everywhere(
        &svc.app_handle,
        "plan-changed",
        serde_json::json!({ "plan_id": plan_id }),
    );
    svc.emit_state_changed();
    Ok(plan)
}

pub fn resume_team_plan(
    svc: &ServiceContext,
    plan_id: &str,
    now_ms: u64,
) -> Result<codeterm_service_core::plan::PlanMode, TeamPlanError> {
    svc.resume_plan_operation(plan_id, now_ms)
        .map(|outcome| outcome.mode)
}

pub fn unarchive_team_plan(
    svc: &ServiceContext,
    plan_id: &str,
    now_ms: u64,
) -> Result<(), TeamPlanError> {
    let mux = svc.mux.read();
    valid_live_plan_team(svc, &mux, plan_id)?;
    svc.plan_store
        .unarchive(plan_id, now_ms)
        .map_err(TeamPlanError::Plan)?;
    drop(mux);
    svc.emit_plan_changed(plan_id);
    Ok(())
}

pub fn transfer_team_plan(
    svc: &ServiceContext,
    plan_id: &str,
    to: codeterm_service_core::plan::OrchestratorRef,
    reason: &str,
    caller_tab_id: String,
    now_ms: u64,
) -> Result<codeterm_service_core::plan::Plan, TeamPlanError> {
    let mut mux = svc.mux.write();
    let plan = transfer_team_plan_core(
        &mut mux,
        &svc.plan_store,
        plan_id,
        to,
        reason,
        TeamMutationCaller::AgentTab(caller_tab_id),
        now_ms,
    )?;
    drop(mux);
    svc.emit_state_changed();
    svc.emit_plan_changed(plan_id);
    Ok(plan)
}

pub fn validate_active_team_plan_bindings(svc: &ServiceContext) -> Result<(), TeamPlanError> {
    let mux = svc.mux.read();
    let mut claimed_teams = std::collections::BTreeMap::<String, String>::new();
    for plan in svc
        .plan_store
        .list_full()
        .into_iter()
        .filter(|plan| plan.status == codeterm_service_core::plan::PlanStatus::Active)
        .filter(|plan| !plan.is_orphaned_active())
    {
        let team_id = valid_live_plan_team(svc, &mux, &plan.id)?;
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

pub fn validate_plan_team_binding(
    svc: &ServiceContext,
    plan_id: &str,
) -> Result<(), TeamPlanError> {
    let Some(plan) = svc.plan_store.get(plan_id) else {
        return Err(TeamPlanError::Plan(PlanError::NotFound));
    };
    if plan.status == codeterm_service_core::plan::PlanStatus::Archived || plan.is_orphaned_active()
    {
        return Ok(());
    }
    let mux = svc.mux.read();
    valid_live_plan_team(svc, &mux, plan_id).map(|_| ())
}

pub fn rename_team_core(
    mux: &mut MultiplexerState,
    team_id: &str,
    name: String,
    caller: &TeamMutationCaller,
) -> Result<Team, String> {
    authorize_team_manager(mux, team_id, caller)?;
    mux.rename_team(team_id, name)
}

pub fn rename_team_with_state_change(
    mux: &mut MultiplexerState,
    events: &codeterm_events::EventBroadcaster,
    team_id: &str,
    name: String,
    caller: &TeamMutationCaller,
) -> Result<Team, String> {
    let team = rename_team_core(mux, team_id, name, caller)?;
    codeterm_service_core::service::publish_mux_state_changed(events);
    Ok(team)
}

#[allow(clippy::too_many_arguments)]
fn commit_team_member_with_plan_takeover<F>(
    mux: &mut MultiplexerState,
    events: &codeterm_events::EventBroadcaster,
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

pub fn adopt_plan_for_team_core(
    mux: &mut MultiplexerState,
    plan_store: &codeterm_service_core::service::plan::PlanStore,
    team_id: &str,
    plan_id: &str,
    caller: TeamMutationCaller,
    now_ms: u64,
) -> Result<codeterm_service_core::plan::Plan, TeamPlanError> {
    authorize_team_mutation(mux, team_id, &caller).map_err(|error| {
        TeamPlanError::Team(format!(
            "Plan adoption requires manager authority in destination Team {team_id}: {error}. \
Safe recovery: old-team member transfers ownership, then new owner adopts."
        ))
    })?;
    let owner_tab_id = team_plan_owner(mux, plan_store, team_id, plan_id, &caller)?;
    let planner = live_team_planner_ref(mux, team_id);
    codeterm_service_core::service::agent_transition::transition_plan_to_orchestrator_core(
        mux,
        plan_store,
        plan_id,
        codeterm_service_core::plan::OrchestratorRef::from_tab(&owner_tab_id),
        team_id,
        &format!("adopted by Team {team_id}"),
        true,
        planner,
        now_ms,
    )
    .map_err(TeamPlanError::Team)
}

pub fn live_team_planner_ref(
    mux: &MultiplexerState,
    team_id: &str,
) -> Option<codeterm_service_core::plan::PlannerRef> {
    let mut planners = mux.all_tabs().filter(|tab| {
        tab.team_id.as_deref() == Some(team_id)
            && tab.role == Some(AgentRole::Manager)
            && tab.role_profile.as_deref()
                == Some(codeterm_types::agent::role_kind::builtins::PLANNER)
            && codeterm_service_core::service::plan_reconcile::tab_has_live_agent(tab)
    });
    let planner = planners.next()?;
    if planners.next().is_some() {
        return None;
    }
    Some(codeterm_service_core::plan::PlannerRef::Tab {
        tab_id: planner.id.clone(),
        session_id: planner.agent_session_id.clone(),
        title: Some(planner.title.clone()),
    })
}

fn transfer_team_plan_core(
    mux: &mut MultiplexerState,
    plan_store: &codeterm_service_core::service::plan::PlanStore,
    plan_id: &str,
    to: codeterm_service_core::plan::OrchestratorRef,
    reason: &str,
    caller: TeamMutationCaller,
    now_ms: u64,
) -> Result<codeterm_service_core::plan::Plan, TeamPlanError> {
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
    codeterm_service_core::service::agent_transition::transition_plan_to_orchestrator_core(
        mux,
        plan_store,
        plan_id,
        to,
        &destination_team_id,
        reason,
        false,
        None,
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
        TeamMutationCaller::AuthenticatedMeshPeer(_)
        | TeamMutationCaller::Human
        | TeamMutationCaller::HumanConfirmedHeaderless
        | TeamMutationCaller::Unattributed => Err(source_denial),
    }
}

/// Resolution is local to the caller's own placement, so several anchors stay
/// unambiguous without a global "there must be exactly one" scan.
pub fn anchor_above(mux: &MultiplexerState, team_id: &str, caller_tab_id: &str) -> Option<String> {
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
pub fn team_plan_owner(
    mux: &MultiplexerState,
    plan_store: &codeterm_service_core::service::plan::PlanStore,
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
    plan_store: &codeterm_service_core::service::plan::PlanStore,
    now_ms: u64,
) -> Result<Vec<String>, codeterm_service_core::service::plan::PlanError> {
    let live_team_ids = mux.list_teams().into_iter().map(|team| team.id).collect();
    plan_store.archive_active_plans_without_live_team(&live_team_ids, now_ms)
}

/// A Team whose anchor lives on another peer is mirrored here for visibility
/// only; mutating it locally would fork it from the authority that owns it.
fn reject_remote_team_mutation(mux: &MultiplexerState, team_id: &str) -> Result<(), String> {
    if !is_remote_team(mux, team_id) {
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
    if matches!(caller, TeamMutationCaller::Unattributed) {
        return Err(format!(
            "Unattributed caller is not authorized to manage Team {team_id}"
        ));
    }
    if matches!(caller, TeamMutationCaller::AuthenticatedMeshPeer(_)) {
        return Err(format!(
            "Authenticated mesh peer is not authorized to manage Team {team_id}"
        ));
    }
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
    reject_remote_team_mutation(mux, team_id).map_err(|error| {
        format!(
            "{}: {error}",
            codeterm_service_core::service::TEAM_CLOSE_REFUSAL_PREFIX
        )
    })?;
    match caller {
        TeamMutationCaller::Unattributed => {
            return Err(format!(
                "{}: Unattributed caller cannot close Team {team_id}; explicit human confirmation is required",
                codeterm_service_core::service::TEAM_CLOSE_REFUSAL_PREFIX
            ));
        }
        TeamMutationCaller::AuthenticatedMeshPeer(_) => return Ok(()),
        TeamMutationCaller::AgentTab(_)
        | TeamMutationCaller::Human
        | TeamMutationCaller::HumanConfirmedHeaderless => {}
    }
    if member_tab_ids.is_empty() {
        return Ok(());
    }
    authorize_team_manager(mux, team_id, caller).map_err(|error| {
        format!(
            "{}: {error}",
            codeterm_service_core::service::TEAM_CLOSE_REFUSAL_PREFIX
        )
    })
}

fn authorize_team_mutation(
    mux: &MultiplexerState,
    team_id: &str,
    caller: &TeamMutationCaller,
) -> Result<(), String> {
    authorize_team_manager(mux, team_id, caller)
}

use codeterm_service_core::service::team_membership::adopt_agent_into_team;

pub mod staffing;
#[cfg(test)]
pub(crate) use staffing::prepare_team_member_spawn_with_provider_validation;
pub use staffing::{
    commit_team_member_spawn, prepare_team_member_spawn, take_over_team_plan, team_member_brief,
    AddTeamMemberParams, AddTeamMemberResult, CartographerAttachLocks, OrchestratorAttachLocks,
};

#[cfg(test)]
mod general_agent_guard_tests;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod transfer_tests;

#[cfg(test)]
mod workspace_tests;
