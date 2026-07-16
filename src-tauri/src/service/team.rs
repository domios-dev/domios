use codeterm_types::team::Team;

use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{AgentRole, PaneBacking};
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
    pub pane_id: String,
    pub team: Team,
}

pub(crate) enum TeamPlanError {
    Team(String),
    Plan(PlanError),
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
        caller_pane_id: &str,
        params: AddTeamMemberParams,
    ) -> Result<AddTeamMemberResult, String> {
        let prepared = {
            let mux = self.mux.read();
            prepare_team_member_spawn_with_provider_validation(
                &mux,
                team_id,
                caller_pane_id,
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
        self.attach_manager_bootstrap(team_id, &mut spawn_params)?;
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
        let pane_id = match spawn {
            Ok(spawn) => spawn.pane_id,
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
                caller_pane_id,
                &pane_id,
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
        params.manager_bootstrap = Some(crate::service::role_identity::ManagerBootstrapInputs {
            role: role.into(),
            team_id: team_id.into(),
            team_name,
            plan_id,
            project,
        });
        Ok(())
    }

    pub(crate) fn rename_team(
        &self,
        team_id: &str,
        name: String,
        caller_pane_id: Option<&str>,
    ) -> Result<Team, String> {
        let mut mux = self.mux.write();
        let team =
            rename_team_with_state_change(&mut mux, &self.events, team_id, name, caller_pane_id)?;
        drop(mux);
        self.emit_state_changed_after_bus_publish();
        Ok(team)
    }

    pub(crate) fn close_team(
        &self,
        team_id: &str,
        caller_pane_id: Option<&str>,
    ) -> Result<Team, String> {
        let (team, cleanup, changed_plan_ids) = {
            let mut mux = self.mux.write();
            close_team_with_state_change(
                &mut mux,
                &self.events,
                &self.plan_store,
                team_id,
                caller_pane_id,
                now_ms(),
            )?
        };
        for plan_id in changed_plan_ids {
            self.emit_plan_changed(&plan_id);
        }
        for pane_id in &cleanup.member_pane_ids {
            self.chat_registry.stop(pane_id);
        }
        for (plugin_id, session_id) in &cleanup.chat_backings {
            let _ = self.chatbackend_close(plugin_id, session_id);
        }
        for pane_id in &cleanup.member_pane_ids {
            self.flag_bound_tasks_for_gone_pane(pane_id, 0);
            self.prompt_detector.lock().remove_pane(pane_id);
            crate::service::structured_prompt::clear_pane(pane_id);
            self.remote_workers.forget_orchestrator(pane_id);
        }
        self.kill_ptys(&cleanup.pty_ids);
        self.emit_state_changed_after_bus_publish();
        Ok(team)
    }

    pub(crate) fn adopt_plan_for_team(
        &self,
        team_id: &str,
        plan_id: &str,
        caller_pane_id: Option<&str>,
        now_ms: u64,
    ) -> Result<crate::plan::Plan, TeamPlanError> {
        if let Err(message) = authorize_team_manager(&self.mux.read(), team_id, caller_pane_id) {
            return Err(TeamPlanError::Team(message));
        }
        let plan = self
            .plan_store
            .bind_plan_to_team(plan_id, team_id, now_ms)
            .map_err(TeamPlanError::Plan)?;
        crate::api::events::emit_everywhere(
            &self.app_handle,
            "plan-changed",
            serde_json::json!({ "plan_id": plan_id }),
        );
        self.emit_state_changed();
        Ok(plan)
    }
}

#[derive(Debug)]
pub(crate) struct CloseTeamCleanup {
    member_pane_ids: Vec<String>,
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
    caller_pane_id: &str,
    params: AddTeamMemberParams,
) -> Result<PreparedTeamMemberSpawn, String> {
    authorize_team_manager(mux, team_id, Some(caller_pane_id))?;
    let (role, role_profile) = team_member_role(&params.role)?;
    Ok(PreparedTeamMemberSpawn {
        expected_role: role.clone(),
        expected_profile: role_profile.clone(),
        spawn_params: crate::service::SpawnAgentParams {
            agent: Some(params.provider),
            title: params.title,
            role: Some(role),
            role_profile,
            parent_pane_id: Some(caller_pane_id.into()),
            task: params.task,
            model: params.model,
            spawned_by_pane_id: Some(caller_pane_id.into()),
            ..Default::default()
        },
    })
}

fn prepare_team_member_spawn_with_provider_validation<F>(
    mux: &MultiplexerState,
    team_id: &str,
    caller_pane_id: &str,
    params: AddTeamMemberParams,
    validate_provider: F,
) -> Result<PreparedTeamMemberSpawn, String>
where
    F: FnOnce(&crate::service::SpawnAgentParams) -> Result<(), String>,
{
    let prepared = prepare_team_member_spawn(mux, team_id, caller_pane_id, params)?;
    validate_provider(&prepared.spawn_params)?;
    Ok(prepared)
}

pub(crate) fn commit_team_member_spawn(
    mux: &mut MultiplexerState,
    events: &crate::api::events::EventBroadcaster,
    team_id: &str,
    caller_pane_id: &str,
    pane_id: &str,
    expected_role: &str,
    expected_profile: &Option<String>,
) -> Result<AddTeamMemberResult, String> {
    let pane = mux
        .get_pane(pane_id)
        .ok_or_else(|| format!("Pane {pane_id} not found after member spawn"))?;
    let invalid_hierarchy = pane.parent_pane_id.as_deref() != Some(caller_pane_id)
        || pane.team_id.as_deref() != Some(team_id)
        || pane.role != AgentRole::from_str_compat(expected_role)
        || pane.role_profile != *expected_profile
        || mux.find_workspace_for_pane(pane_id) != mux.find_workspace_for_pane(caller_pane_id);
    if invalid_hierarchy {
        mux.close_pane(pane_id)?;
        return Err("Team member spawn did not preserve the manager hierarchy".into());
    }
    let team = mux
        .get_team(team_id)
        .cloned()
        .ok_or_else(|| format!("Team {team_id} not found after member spawn"))?;
    crate::service::publish_mux_state_changed(events);
    Ok(AddTeamMemberResult {
        pane_id: pane_id.into(),
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
    caller_pane_id: Option<&str>,
) -> Result<Team, String> {
    authorize_team_manager(mux, team_id, caller_pane_id)?;
    mux.rename_team(team_id, name)
}

pub(crate) fn rename_team_with_state_change(
    mux: &mut MultiplexerState,
    events: &crate::api::events::EventBroadcaster,
    team_id: &str,
    name: String,
    caller_pane_id: Option<&str>,
) -> Result<Team, String> {
    let team = rename_team_core(mux, team_id, name, caller_pane_id)?;
    crate::service::publish_mux_state_changed(events);
    Ok(team)
}

pub(crate) fn close_team_with_state_change(
    mux: &mut MultiplexerState,
    events: &crate::api::events::EventBroadcaster,
    plan_store: &crate::service::plan::PlanStore,
    team_id: &str,
    caller_pane_id: Option<&str>,
    now_ms: u64,
) -> Result<(Team, CloseTeamCleanup, Vec<String>), String> {
    close_team_with_state_change_using(
        mux,
        events,
        plan_store,
        team_id,
        caller_pane_id,
        now_ms,
        |mux, pane_id| mux.close_pane_for_team_close(pane_id),
    )
}

fn close_team_with_state_change_using<F>(
    mux: &mut MultiplexerState,
    events: &crate::api::events::EventBroadcaster,
    plan_store: &crate::service::plan::PlanStore,
    team_id: &str,
    caller_pane_id: Option<&str>,
    now_ms: u64,
    mut close_member: F,
) -> Result<(Team, CloseTeamCleanup, Vec<String>), String>
where
    F: FnMut(&mut MultiplexerState, &str) -> Result<Vec<String>, String>,
{
    authorize_team_manager(mux, team_id, caller_pane_id)?;
    let member_pane_ids: Vec<String> = mux
        .snapshot()
        .panes
        .values()
        .filter(|pane| pane.team_id.as_deref() == Some(team_id))
        .map(|pane| pane.id.clone())
        .collect();
    let chat_backings = member_pane_ids
        .iter()
        .filter_map(|pane_id| match mux.get_pane(pane_id)?.backing.clone() {
            PaneBacking::ChatBackend {
                plugin_id,
                session_id,
            } => Some((plugin_id, session_id)),
            PaneBacking::Pty => None,
        })
        .collect();
    let mut pty_ids = Vec::new();
    let mut closed_roots = Vec::new();
    let mut mux_transaction = mux.begin_team_close_transaction();
    for pane_id in &member_pane_ids {
        if mux_transaction.state_mut().get_pane(pane_id).is_some() {
            pty_ids.extend(close_member(mux_transaction.state_mut(), pane_id)?);
            closed_roots.push(pane_id.clone());
        }
    }
    let team = mux_transaction
        .state_mut()
        .remove_team(team_id)
        .ok_or_else(|| format!("Team {team_id} not found"))?;
    let plan_transaction = plan_store
        .detach_and_park_team_plans_transaction(team_id, now_ms)
        .map_err(|error| error.to_string())?;
    let changed_plan_ids = plan_transaction.changed_plan_ids().to_vec();
    mux_transaction.commit(mux, &closed_roots);
    crate::service::publish_mux_state_changed(events);
    Ok((
        team,
        CloseTeamCleanup {
            member_pane_ids,
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
    plan_store.detach_and_park_dangling_team_plans(&live_team_ids, now_ms)
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
    caller_pane_id: Option<&str>,
) -> Result<(), String> {
    if mux.get_team(team_id).is_none() {
        return Err(format!("Team {team_id} not found"));
    }
    let Some(caller_pane_id) = caller_pane_id else {
        return Ok(());
    };
    let caller = mux
        .get_pane(caller_pane_id)
        .ok_or_else(|| format!("Caller pane {caller_pane_id} not found"))?;
    if caller.team_id.as_deref() != Some(team_id) {
        return Err(format!(
            "Caller pane {caller_pane_id} is not a member of Team {team_id}"
        ));
    }
    let manager = caller.role == Some(AgentRole::Orchestrator)
        || (caller.role == Some(AgentRole::Manager)
            && caller.role_profile.as_deref() == Some("planner"));
    if !manager {
        return Err(format!(
            "Caller pane {caller_pane_id} is not authorized to manage Team {team_id}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::api::events::SystemEvent;

    fn team_members() -> (MultiplexerState, String, String, String, String) {
        let (mut mux, orchestrator) = MultiplexerState::new();
        let team_id = mux
            .resolve_or_mint_team_for_parent(&orchestrator)
            .unwrap()
            .team_id;
        let workspace_id = mux.find_workspace_for_pane(&orchestrator).unwrap();
        let planner = mux.create_tab(&workspace_id).unwrap().1;
        mux.set_pane_role(&planner, AgentRole::Manager, Some(&orchestrator))
            .unwrap();
        mux.get_pane_mut(&planner).unwrap().role_profile = Some("planner".into());
        let worker = mux.create_tab(&workspace_id).unwrap().1;
        mux.set_pane_role(&worker, AgentRole::Worker, Some(&orchestrator))
            .unwrap();
        (mux, team_id, orchestrator, planner, worker)
    }

    #[test]
    fn team_manager_permission_allows_orchestrator_and_planner() {
        let (mux, team_id, orchestrator, planner, _) = team_members();
        assert!(authorize_team_manager(&mux, &team_id, None).is_ok());
        assert!(authorize_team_manager(&mux, &team_id, Some(&orchestrator)).is_ok());
        assert!(authorize_team_manager(&mux, &team_id, Some(&planner)).is_ok());
    }

    #[test]
    fn mux_rename_team_rejects_worker_and_foreign_pane() {
        let (mut mux, team_id, _, _, worker) = team_members();
        assert!(authorize_team_manager(&mux, &team_id, Some(&worker)).is_err());
        let foreign = mux.create_pane();
        assert!(authorize_team_manager(&mux, &team_id, Some(&foreign)).is_err());
    }

    #[test]
    fn team_member_role_accepts_only_worker_or_planner() {
        assert_eq!(team_member_role("worker").unwrap(), ("Worker".into(), None));
        assert_eq!(
            team_member_role("planner").unwrap(),
            ("Manager".into(), Some("planner".into()))
        );
        assert!(team_member_role("manager").is_err());
    }

    #[test]
    fn close_team_removes_registry_and_members_with_one_state_change() {
        let (mut mux, team_id, orchestrator, _, _) = team_members();
        let plan_store = crate::service::plan::PlanStore::new();
        let workspace_id = mux.find_workspace_for_pane(&orchestrator).unwrap();
        let worker_a = spawn_fixture_member(
            &mut mux,
            &workspace_id,
            &orchestrator,
            AgentRole::Worker,
            None,
        );
        let worker_b = spawn_fixture_member(
            &mut mux,
            &workspace_id,
            &orchestrator,
            AgentRole::Worker,
            None,
        );
        let events = crate::api::events::EventBroadcaster::new();
        let mut subscription = events.subscribe();

        let (closed, _, _) = close_team_with_state_change(
            &mut mux,
            &events,
            &plan_store,
            &team_id,
            Some(&orchestrator),
            1,
        )
        .unwrap();

        assert_eq!(closed.id, team_id);
        assert!(mux.get_team(&team_id).is_none());
        assert!(mux.get_pane(&orchestrator).is_none());
        assert!(mux.get_pane(&worker_a).is_none());
        assert!(mux.get_pane(&worker_b).is_none());
        assert!(matches!(
            subscription.try_recv().unwrap().event,
            SystemEvent::MuxStateChanged
        ));
        assert!(matches!(
            subscription.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
    }

    #[test]
    fn close_team_parks_and_detaches_bound_plan() {
        let dir = tempfile::tempdir().unwrap();
        let plan_store = crate::service::plan::PlanStore::new_for_test(dir.path());
        let (mut mux, team_id, orchestrator, _, _) = team_members();
        let plan = plan_store
            .create_plan_for_team(
                "Bound".into(),
                dir.path().display().to_string(),
                crate::plan::OrchestratorRef::from_pane(&orchestrator),
                Some(team_id.clone()),
                vec![],
                1,
            )
            .unwrap();
        let events = crate::api::events::EventBroadcaster::new();
        let (_, _, changed_plan_ids) = close_team_with_state_change(
            &mut mux,
            &events,
            &plan_store,
            &team_id,
            Some(&orchestrator),
            2,
        )
        .unwrap();

        let parked = plan_store.get(&plan.id).unwrap();
        assert!(mux.get_team(&team_id).is_none());
        assert_eq!(changed_plan_ids, vec![plan.id.clone()]);
        assert_eq!(parked.team_id, None);
        assert_eq!(parked.mode, crate::plan::PlanMode::Paused);
        assert!(plan_store
            .list_index()
            .iter()
            .any(|entry| entry.plan_id == plan.id));
    }

    #[test]
    fn close_team_staging_failure_leaves_mux_and_plan_unchanged_without_events() {
        let dir = tempfile::tempdir().unwrap();
        let plan_store = crate::service::plan::PlanStore::new_for_test(dir.path());
        let (mut mux, team_id, orchestrator, planner, worker) = team_members();
        let workspace_id = mux.find_workspace_for_pane(&orchestrator).unwrap();
        let independent = mux.create_tab(&workspace_id).unwrap().1;
        mux.get_pane_mut(&independent).unwrap().team_id = Some(team_id.clone());
        let plan = plan_store
            .create_plan_for_team(
                "Bound".into(),
                dir.path().display().to_string(),
                crate::plan::OrchestratorRef::from_pane(&orchestrator),
                Some(team_id.clone()),
                vec![],
                1,
            )
            .unwrap();
        let before_plan = plan_store.get(&plan.id).unwrap();
        let before_members = [
            orchestrator.clone(),
            planner.clone(),
            worker.clone(),
            independent,
        ];
        let before_snapshot = snapshot_without_generation(&mux);
        let events = crate::api::events::EventBroadcaster::new();
        let mut subscription = events.subscribe();

        let error = close_team_with_state_change_using(
            &mut mux,
            &events,
            &plan_store,
            &team_id,
            Some(&orchestrator),
            2,
            {
                let mut calls = 0;
                move |staged_mux, pane_id| {
                    calls += 1;
                    if calls == 2 {
                        Err("forced mux close failure".into())
                    } else {
                        staged_mux.close_pane_for_team_close(pane_id)
                    }
                }
            },
        )
        .unwrap_err();

        assert_eq!(error, "forced mux close failure");
        assert!(mux.get_team(&team_id).is_some());
        assert!(before_members
            .iter()
            .all(|pane_id| mux.get_pane(pane_id).is_some()));
        assert_eq!(snapshot_without_generation(&mux), before_snapshot);
        assert_eq!(plan_store.get(&plan.id).unwrap(), before_plan);
        assert!(matches!(
            subscription.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
    }

    fn snapshot_without_generation(mux: &MultiplexerState) -> serde_json::Value {
        let mut snapshot = serde_json::to_value(mux.snapshot()).unwrap();
        snapshot["generation"] = serde_json::Value::Null;
        snapshot
    }

    #[test]
    fn dangling_binding_sweep_parks_orphan_plans_once() {
        let dir = tempfile::tempdir().unwrap();
        let plan_store = crate::service::plan::PlanStore::new_for_test(dir.path());
        let (mux, _, orchestrator, _, _) = team_members();
        let plan = plan_store
            .create_plan_for_team(
                "Orphan".into(),
                dir.path().display().to_string(),
                crate::plan::OrchestratorRef::from_pane(&orchestrator),
                Some("team_missing".into()),
                vec![],
                1,
            )
            .unwrap();

        assert_eq!(
            reconcile_dangling_team_plan_bindings(&mux, &plan_store, 2).unwrap(),
            vec![plan.id.clone()]
        );
        let repaired = plan_store.get(&plan.id).unwrap();
        assert_eq!(repaired.team_id, None);
        assert_eq!(repaired.mode, crate::plan::PlanMode::Paused);
        assert!(reconcile_dangling_team_plan_bindings(&mux, &plan_store, 3)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn ordinary_close_preserves_team_but_explicit_close_removes_it() {
        let (mut mux, team_id, orchestrator, planner, worker) = team_members();
        let plan_store = crate::service::plan::PlanStore::new();
        mux.close_pane(&orchestrator).unwrap();
        mux.close_pane(&planner).unwrap();
        mux.close_pane(&worker).unwrap();
        assert!(mux.get_team(&team_id).is_some());
        let events = crate::api::events::EventBroadcaster::new();
        close_team_with_state_change(&mut mux, &events, &plan_store, &team_id, None, 1).unwrap();
        assert!(mux.get_team(&team_id).is_none());
    }

    #[test]
    fn close_team_unknown_and_repeat_are_structured_errors() {
        let (mut mux, team_id, orchestrator, _, _) = team_members();
        let plan_store = crate::service::plan::PlanStore::new();
        let events = crate::api::events::EventBroadcaster::new();
        assert!(close_team_with_state_change(
            &mut mux,
            &events,
            &plan_store,
            "team_missing",
            Some(&orchestrator),
            1,
        )
        .unwrap_err()
        .contains("not found"));
        close_team_with_state_change(
            &mut mux,
            &events,
            &plan_store,
            &team_id,
            Some(&orchestrator),
            1,
        )
        .unwrap();
        assert!(close_team_with_state_change(
            &mut mux,
            &events,
            &plan_store,
            &team_id,
            Some(&orchestrator),
            1,
        )
        .unwrap_err()
        .contains("not found"));
    }

    #[test]
    fn close_team_removes_chat_backend_member_and_registry() {
        let (mut mux, team_id, orchestrator, _, worker) = team_members();
        let plan_store = crate::service::plan::PlanStore::new();
        mux.get_pane_mut(&worker).unwrap().backing = PaneBacking::ChatBackend {
            plugin_id: "chat-backend".into(),
            session_id: "session-1".into(),
        };
        let events = crate::api::events::EventBroadcaster::new();

        let (_, cleanup, _) = close_team_with_state_change(
            &mut mux,
            &events,
            &plan_store,
            &team_id,
            Some(&orchestrator),
            1,
        )
        .unwrap();

        assert!(mux.get_team(&team_id).is_none());
        assert!(mux.get_pane(&worker).is_none());
        assert_eq!(
            cleanup.chat_backings,
            vec![("chat-backend".into(), "session-1".into())]
        );
    }

    fn spawn_fixture_member(
        mux: &mut MultiplexerState,
        workspace_id: &str,
        parent: &str,
        role: AgentRole,
        profile: Option<&str>,
    ) -> String {
        let pane_id = mux.create_tab(workspace_id).unwrap().1;
        mux.set_pane_role(&pane_id, role, Some(parent)).unwrap();
        mux.get_pane_mut(&pane_id).unwrap().role_profile = profile.map(str::to_string);
        pane_id
    }

    #[test]
    fn add_team_member_uses_existing_manager_and_team() {
        let (mut mux, team_id, manager, _, _) = team_members();
        let workspace_id = mux.find_workspace_for_pane(&manager).unwrap();
        let before_count = mux.team_count();
        let prepared = prepare_team_member_spawn(
            &mux,
            &team_id,
            &manager,
            AddTeamMemberParams {
                provider: "spawn-capable".into(),
                role: "worker".into(),
                title: None,
                task: None,
                model: None,
            },
        )
        .unwrap();
        let pane_id =
            spawn_fixture_member(&mut mux, &workspace_id, &manager, AgentRole::Worker, None);
        let events = crate::api::events::EventBroadcaster::new();
        let result = commit_team_member_spawn(
            &mut mux,
            &events,
            &team_id,
            &manager,
            &pane_id,
            &prepared.expected_role,
            &prepared.expected_profile,
        )
        .unwrap();
        let pane = mux.get_pane(&result.pane_id).unwrap();
        assert_eq!(pane.parent_pane_id.as_deref(), Some(manager.as_str()));
        assert_eq!(pane.team_id.as_deref(), Some(team_id.as_str()));
        assert_eq!(pane.role, Some(AgentRole::Worker));
        assert_eq!(
            mux.find_workspace_for_pane(&result.pane_id).as_deref(),
            Some(workspace_id.as_str())
        );
        assert_eq!(mux.team_count(), before_count);
        assert_eq!(result.team.id, team_id);
    }

    #[test]
    fn add_team_member_sets_exact_planner_role_and_emits_one_state_change() {
        let (mut mux, team_id, manager, _, _) = team_members();
        let workspace_id = mux.find_workspace_for_pane(&manager).unwrap();
        let prepared = prepare_team_member_spawn(
            &mux,
            &team_id,
            &manager,
            AddTeamMemberParams {
                provider: "spawn-capable".into(),
                role: "planner".into(),
                title: None,
                task: None,
                model: None,
            },
        )
        .unwrap();
        let pane_id = spawn_fixture_member(
            &mut mux,
            &workspace_id,
            &manager,
            AgentRole::Manager,
            Some("planner"),
        );
        let events = crate::api::events::EventBroadcaster::new();
        let mut subscription = events.subscribe();
        let result = commit_team_member_spawn(
            &mut mux,
            &events,
            &team_id,
            &manager,
            &pane_id,
            &prepared.expected_role,
            &prepared.expected_profile,
        )
        .unwrap();
        let pane = mux.get_pane(&result.pane_id).unwrap();
        assert_eq!(pane.role, Some(AgentRole::Manager));
        assert_eq!(pane.role_profile.as_deref(), Some("planner"));
        assert!(matches!(
            subscription.try_recv().unwrap().event,
            SystemEvent::MuxStateChanged
        ));
        assert!(matches!(
            subscription.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
    }

    #[test]
    fn team_member_role_accepts_orchestrator() {
        assert_eq!(
            team_member_role("orchestrator").unwrap(),
            ("Orchestrator".into(), None)
        );
    }

    #[test]
    fn add_team_member_rejects_worker_foreign_and_unspawnable_without_mutation() {
        let (mut mux, team_id, manager, _, worker) = team_members();
        let workspace_id = mux.find_workspace_for_pane(&manager).unwrap();
        let (worker, foreign) = {
            let foreign = mux.create_tab(&workspace_id).unwrap().1;
            (worker, foreign)
        };
        let before_panes = mux.pane_count_in_workspace(&workspace_id);
        let before_teams = mux.team_count();
        for caller in [&worker, &foreign] {
            assert!(prepare_team_member_spawn_with_provider_validation(
                &mux,
                &team_id,
                caller,
                AddTeamMemberParams {
                    provider: "unknown-provider".into(),
                    role: "worker".into(),
                    title: None,
                    task: None,
                    model: None
                },
                |_| Ok(()),
            )
            .is_err());
        }
        for provider in ["unknown-provider", "incapable-provider"] {
            let error = prepare_team_member_spawn_with_provider_validation(
                &mux,
                &team_id,
                &manager,
                AddTeamMemberParams {
                    provider: provider.into(),
                    role: "worker".into(),
                    title: None,
                    task: None,
                    model: None,
                },
                |spawn| {
                    Err(format!(
                        "provider {} is not spawn-capable",
                        spawn.agent.as_deref().unwrap_or_default()
                    ))
                },
            )
            .unwrap_err();
            assert!(error.contains(provider));
        }
        let expected = prepare_team_member_spawn_with_provider_validation(
            &mux,
            &team_id,
            &manager,
            AddTeamMemberParams {
                provider: "spawn-capable".into(),
                role: "worker".into(),
                title: None,
                task: None,
                model: None,
            },
            |_| Ok(()),
        )
        .unwrap();
        let pane_id =
            spawn_fixture_member(&mut mux, &workspace_id, &manager, AgentRole::Worker, None);
        mux.get_pane_mut(&pane_id).unwrap().team_id = None;
        let events = crate::api::events::EventBroadcaster::new();
        let mut subscription = events.subscribe();
        assert!(commit_team_member_spawn(
            &mut mux,
            &events,
            &team_id,
            &manager,
            &pane_id,
            &expected.expected_role,
            &expected.expected_profile
        )
        .is_err());
        assert!(mux.get_pane(&pane_id).is_none());
        assert!(matches!(
            subscription.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
        assert_eq!(mux.pane_count_in_workspace(&workspace_id), before_panes);
        assert_eq!(mux.team_count(), before_teams);
    }
}
