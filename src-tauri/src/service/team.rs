use codeterm_types::team::Team;

use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::AgentRole;
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
        let pane_id = self
            .spawn_agent_with_state_change(prepared.spawn_params, None, false)
            .await?
            .pane_id;
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
        }?;
        self.emit_state_changed_after_bus_publish();
        Ok(result)
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
        other => Err(format!(
            "Team member role must be worker or planner, got '{other}'"
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
        let planner = mux.create_pane();
        mux.set_pane_role(&planner, AgentRole::Manager, Some(&orchestrator))
            .unwrap();
        mux.get_pane_mut(&planner).unwrap().role_profile = Some("planner".into());
        let worker = mux.create_pane();
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
