use serde::Serialize;
use tauri::State;

use crate::multiplexer::state::{MultiplexerSnapshot, WorkspaceInfo};
use crate::service::{BatchOp, ServiceState};
use codeterm_types::team::Team;

#[derive(Debug, Clone, Serialize)]
pub struct TabInfo {
    pub id: String,
    pub title: String,
}

#[derive(serde::Serialize)]
pub struct OrphanedRestoreInfo {
    pub workspace_id: String,
    pub tab_id: String,
}

/// Returns the full multiplexer state for frontend hydration (read-only).
#[tauri::command]
pub fn mux_get_state(svc: State<'_, ServiceState>) -> Result<MultiplexerSnapshot, String> {
    Ok(svc.snapshot_with_reconciled_status())
}

/// Create a new workspace.
#[tauri::command]
pub fn mux_create_workspace(
    svc: State<'_, ServiceState>,
    name: String,
    cwd: Option<String>,
    activate: Option<bool>,
) -> Result<WorkspaceInfo, String> {
    Ok(svc.create_workspace(name, cwd, activate.unwrap_or(false)))
}

/// Switch the active workspace.
#[tauri::command]
pub fn mux_set_active_workspace(
    svc: State<'_, ServiceState>,
    workspace_id: String,
) -> Result<(), String> {
    svc.focus_workspace(&workspace_id)
}

/// Close a workspace and kill all its PTYs.
#[tauri::command]
pub fn mux_close_workspace(
    svc: State<'_, ServiceState>,
    workspace_id: String,
) -> Result<(), String> {
    svc.close_workspace(&workspace_id)
}

/// Rename a workspace.
#[tauri::command]
pub fn mux_rename_workspace(
    svc: State<'_, ServiceState>,
    workspace_id: String,
    name: String,
) -> Result<(), String> {
    svc.rename_workspace(&workspace_id, &name)
}

#[cfg(test)]
pub fn mux_rename_team_core(
    mux: &mut crate::multiplexer::state::MultiplexerState,
    team_id: String,
    name: String,
    caller_tab_id: Option<String>,
) -> Result<Team, String> {
    let caller_tab_id = require_team_rename_caller(caller_tab_id)?;
    crate::service::rename_team_core(mux, &team_id, name, Some(&caller_tab_id))
}

pub(crate) fn mux_rename_team_service_core(
    svc: &ServiceState,
    team_id: String,
    name: String,
    caller_tab_id: Option<String>,
) -> Result<Team, String> {
    let caller_tab_id = require_team_rename_caller(caller_tab_id)?;
    svc.rename_team(&team_id, name, Some(&caller_tab_id))
}

fn require_team_rename_caller(caller_tab_id: Option<String>) -> Result<String, String> {
    caller_tab_id.ok_or_else(|| "callerTabId is required to rename a Team".into())
}

/// Rename a Team through the service boundary so authorization and the state
/// change event remain identical for desktop and web callers.
#[tauri::command]
pub fn mux_rename_team(
    svc: State<'_, ServiceState>,
    team_id: String,
    name: String,
    caller_tab_id: String,
) -> Result<Team, String> {
    mux_rename_team_service_core(svc.inner(), team_id, name, Some(caller_tab_id))
}

pub(crate) fn mux_close_team_service_core(
    svc: &ServiceState,
    team_id: String,
    caller_tab_id: Option<String>,
) -> Result<Team, String> {
    let caller_tab_id =
        caller_tab_id.ok_or_else(|| "callerTabId is required to close a Team".to_string())?;
    svc.close_team(&team_id, Some(&caller_tab_id))
}

#[tauri::command]
pub fn mux_close_team(
    svc: State<'_, ServiceState>,
    team_id: String,
    caller_tab_id: String,
) -> Result<Team, String> {
    mux_close_team_service_core(svc.inner(), team_id, Some(caller_tab_id))
}

pub(crate) async fn mux_add_team_member_service_core(
    svc: &ServiceState,
    team_id: String,
    provider: String,
    role: String,
    caller_tab_id: Option<String>,
    title: Option<String>,
    task: Option<String>,
    model: Option<String>,
) -> Result<crate::service::AddTeamMemberResult, String> {
    let caller_tab_id =
        caller_tab_id.ok_or_else(|| "callerTabId is required to add a Team member".to_string())?;
    svc.add_team_member(
        &team_id,
        &caller_tab_id,
        crate::service::AddTeamMemberParams {
            provider,
            role,
            title,
            task,
            model,
        },
    )
    .await
}

#[tauri::command]
pub async fn mux_add_team_member(
    svc: State<'_, ServiceState>,
    team_id: String,
    provider: String,
    role: String,
    caller_tab_id: String,
    title: Option<String>,
    task: Option<String>,
    model: Option<String>,
) -> Result<crate::service::AddTeamMemberResult, String> {
    mux_add_team_member_service_core(
        svc.inner(),
        team_id,
        provider,
        role,
        Some(caller_tab_id),
        title,
        task,
        model,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::api::events::SystemEvent;
    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::AgentRole;

    fn team_members() -> (MultiplexerState, String, String, String, String) {
        let (mut mux, orchestrator) = MultiplexerState::new();
        let team_id = mux
            .resolve_or_mint_team_for_parent(&orchestrator)
            .unwrap()
            .team_id;
        let workspace_id = mux.find_workspace_for_tab(&orchestrator).unwrap();
        let planner = mux.create_tab(&workspace_id).unwrap();
        mux.set_tab_role(&planner, AgentRole::Manager, Some(&orchestrator))
            .unwrap();
        mux.get_tab_mut(&planner).unwrap().role_profile = Some("planner".into());
        let worker = mux.create_tab(&workspace_id).unwrap();
        mux.set_tab_role(&worker, AgentRole::Worker, Some(&orchestrator))
            .unwrap();
        (mux, team_id, orchestrator, planner, worker)
    }

    #[test]
    fn mux_rename_team_rejects_missing_caller() {
        assert_eq!(
            require_team_rename_caller(None).unwrap_err(),
            "callerTabId is required to rename a Team"
        );
    }

    #[test]
    fn mux_rename_team_core_allows_orchestrator_and_planner() {
        let (mut mux, team_id, orchestrator, planner, _) = team_members();

        let by_orchestrator = mux_rename_team_core(
            &mut mux,
            team_id.clone(),
            "Orchestrated".into(),
            Some(orchestrator),
        )
        .unwrap();
        assert_eq!(by_orchestrator.name.as_deref(), Some("Orchestrated"));

        let by_planner =
            mux_rename_team_core(&mut mux, team_id.clone(), "Planned".into(), Some(planner))
                .unwrap();
        assert_eq!(by_planner.name.as_deref(), Some("Planned"));
        assert_eq!(
            mux.get_team(&team_id).unwrap().name.as_deref(),
            Some("Planned")
        );
    }

    #[test]
    fn mux_rename_team_core_rejects_worker_and_foreign_tab() {
        let (mut mux, team_id, _, _, worker) = team_members();
        let workspace_id = mux.find_workspace_for_tab(&worker).unwrap();
        let foreign = mux.create_tab(&workspace_id).unwrap();

        assert!(mux_rename_team_core(
            &mut mux,
            team_id.clone(),
            "Denied worker".into(),
            Some(worker),
        )
        .is_err());
        assert!(mux_rename_team_core(
            &mut mux,
            team_id.clone(),
            "Denied foreign".into(),
            Some(foreign),
        )
        .is_err());
        assert_eq!(mux.get_team(&team_id).unwrap().name, None);
    }

    #[test]
    fn team_rename_emits_exactly_one_mux_state_changed() {
        let (mut mux, team_id, orchestrator, _, _) = team_members();
        let events = crate::api::events::EventBroadcaster::new();
        let mut receiver = events.subscribe();

        crate::service::rename_team_with_state_change(
            &mut mux,
            &events,
            &team_id,
            "Renamed".into(),
            Some(&orchestrator),
        )
        .unwrap();
        assert_eq!(
            mux.get_team(&team_id).unwrap().name.as_deref(),
            Some("Renamed")
        );

        assert!(matches!(
            receiver.try_recv().unwrap().event,
            SystemEvent::MuxStateChanged
        ));
        assert!(matches!(
            receiver.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
    }
}

/// Move a tab from one workspace to another.
#[tauri::command]
pub fn mux_move_tab(
    svc: State<'_, ServiceState>,
    tab_id: String,
    from_workspace_id: String,
    to_workspace_id: String,
) -> Result<(), String> {
    svc.move_tab(&tab_id, &from_workspace_id, &to_workspace_id)
}

/// Reorder the top-level tabs of a workspace (within-group only).
#[tauri::command]
pub fn mux_reorder_tab(
    svc: State<'_, ServiceState>,
    workspace_id: String,
    ordered_tab_ids: Vec<String>,
) -> Result<(), String> {
    svc.reorder_tab(&workspace_id, &ordered_tab_ids)
}

/// Reorder the child tabs of a parent tab (within its sibling group only).
#[tauri::command]
pub fn mux_reorder_child(
    svc: State<'_, ServiceState>,
    parent_tab_id: String,
    ordered_tab_ids: Vec<String>,
) -> Result<(), String> {
    svc.reorder_child(&parent_tab_id, &ordered_tab_ids)
}

/// Reorder workspaces.
#[tauri::command]
pub fn mux_reorder_workspace(
    svc: State<'_, ServiceState>,
    ordered_workspace_ids: Vec<String>,
) -> Result<(), String> {
    svc.reorder_workspace(&ordered_workspace_ids)
}

/// Create a new tab in a workspace.
#[tauri::command]
pub fn mux_create_tab(
    svc: State<'_, ServiceState>,
    workspace_id: String,
    eager: Option<bool>,
) -> Result<TabInfo, String> {
    let tab_id = svc.create_tab(&workspace_id)?;
    if eager.unwrap_or(false) {
        svc.ensure_tab_pty_for_send(&tab_id)?;
    }
    let title = svc.mux.read().display_label_for_tab(&tab_id);
    Ok(TabInfo {
        id: tab_id.clone(),
        title,
    })
}

/// Switch the active tab in a workspace.
#[tauri::command]
pub fn mux_set_active_tab(
    svc: State<'_, ServiceState>,
    workspace_id: String,
    tab_id: String,
) -> Result<(), String> {
    svc.set_active_tab(&workspace_id, &tab_id)
}

/// Atomically switch active workspace and active tab.
#[tauri::command]
pub fn mux_set_active_tab_and_workspace(
    svc: State<'_, ServiceState>,
    workspace_id: String,
    tab_id: String,
) -> Result<(), String> {
    svc.set_active_tab_and_workspace(&workspace_id, &tab_id)
}

/// Close a tab and kill its PTY (cascade-closes children).
#[tauri::command]
pub fn mux_close_tab(svc: State<'_, ServiceState>, tab_id: String) -> Result<(), String> {
    svc.close_tab(&tab_id)
}

/// Reopen a recently-closed tab (browser-style).
#[tauri::command]
pub fn mux_reopen_closed_tab(
    svc: State<'_, ServiceState>,
    workspace_id: String,
    index: Option<usize>,
) -> Result<TabInfo, String> {
    let tab_id = svc.reopen_closed_tab(&workspace_id, index)?;
    let title = svc.mux.read().display_label_for_tab(&tab_id);
    Ok(TabInfo { id: tab_id, title })
}

/// Reopen a whole closed group by id.
#[tauri::command]
pub fn mux_reopen_closed_group(
    svc: State<'_, ServiceState>,
    group_id: String,
) -> Result<OrphanedRestoreInfo, String> {
    let (workspace_id, tab_id) = svc.reopen_closed_group(&group_id)?;
    Ok(OrphanedRestoreInfo {
        workspace_id,
        tab_id,
    })
}

/// Reopen a tab from the orphaned ring.
#[tauri::command]
pub fn mux_restore_orphaned_tab(
    svc: State<'_, ServiceState>,
    tab_id: String,
) -> Result<OrphanedRestoreInfo, String> {
    let (workspace_id, tab_id) = svc.reopen_orphaned_tab(tab_id.as_str())?;
    Ok(OrphanedRestoreInfo {
        workspace_id,
        tab_id,
    })
}

/// Set the focused tab.
#[tauri::command]
pub fn mux_set_focused_tab(svc: State<'_, ServiceState>, tab_id: String) -> Result<(), String> {
    svc.focus_tab(&tab_id)
}

/// Link a PTY to a tab.
#[tauri::command]
pub fn mux_link_pty(
    svc: State<'_, ServiceState>,
    tab_id: String,
    pty_id: String,
) -> Result<String, String> {
    Ok(svc.link_pty(&tab_id, pty_id))
}

/// Rename a tab (User-level — highest priority, blocks auto-rename).
#[tauri::command]
pub fn mux_rename_tab(
    svc: State<'_, ServiceState>,
    tab_id: String,
    title: String,
) -> Result<(), String> {
    svc.rename_tab(&tab_id, &title)
}

/// Update the working directory of a tab (called from OSC 7 handler).
#[tauri::command]
pub fn mux_update_tab_cwd(
    svc: State<'_, ServiceState>,
    tab_id: String,
    cwd: String,
) -> Result<(), String> {
    svc.update_tab_cwd(&tab_id, cwd);
    Ok(())
}

/// Update the agent type of a tab (called when Claude Code is detected).
#[tauri::command]
pub fn mux_set_tab_agent_type(
    svc: State<'_, ServiceState>,
    tab_id: String,
    agent_type: Option<String>,
) -> Result<(), String> {
    svc.set_tab_agent_type(&tab_id, agent_type);
    Ok(())
}

/// Release the `agent_type_locked` flag on a tab.
#[tauri::command]
pub fn mux_unlock_tab_agent_type(
    svc: State<'_, ServiceState>,
    tab_id: String,
) -> Result<(), String> {
    svc.unlock_tab_agent_type(&tab_id);
    Ok(())
}

/// Set chat view state on a tab.
#[tauri::command]
pub fn mux_set_chat_view(
    svc: State<'_, ServiceState>,
    tab_id: String,
    enabled: bool,
) -> Result<(), String> {
    svc.set_chat_view(&tab_id, enabled);
    Ok(())
}

/// Detect agent session IDs for a given working directory.
#[tauri::command]
pub fn detect_claude_session_id(
    svc: State<'_, ServiceState>,
    cwd: String,
    exclude: Option<Vec<String>>,
    max_age_secs: Option<u64>,
) -> Result<Option<String>, String> {
    detect_agent_session_id(&svc.agent_registry, "claude", &cwd, exclude, max_age_secs)
}

/// Provider-agnostic version of `detect_claude_session_id`.
#[tauri::command]
pub fn detect_tab_session_id(
    svc: State<'_, ServiceState>,
    provider_id: String,
    cwd: String,
    exclude: Option<Vec<String>>,
    max_age_secs: Option<u64>,
) -> Result<Option<String>, String> {
    detect_agent_session_id(
        &svc.agent_registry,
        &provider_id,
        &cwd,
        exclude,
        max_age_secs,
    )
}

pub fn detect_agent_session_id(
    registry: &crate::agent::registry::AgentRegistry,
    agent_type: &str,
    cwd: &str,
    exclude: Option<Vec<String>>,
    max_age_secs: Option<u64>,
) -> Result<Option<String>, String> {
    let excluded = exclude.unwrap_or_default();
    let max_age = max_age_secs.map(std::time::Duration::from_secs);
    registry
        .with_provider(agent_type, |p| p.detect_session_id(cwd, &excluded, max_age))
        .ok_or_else(|| format!("Unknown agent type: {}", agent_type))
        .map(|opt| opt)
}

/// Store a detected agent session ID on a tab.
#[tauri::command]
pub fn mux_set_claude_session_id(
    svc: State<'_, ServiceState>,
    tab_id: String,
    session_id: Option<String>,
) -> Result<(), String> {
    svc.set_agent_session_id(&tab_id, session_id);
    Ok(())
}

/// Build the shell launch command for a given agent provider.
#[tauri::command]
pub fn build_agent_launch_command(
    svc: State<'_, ServiceState>,
    agent_id: String,
    skip_permissions: bool,
) -> Result<String, String> {
    svc.require_agent_provider_ready(&agent_id)?;
    let model_args = crate::service::spawn::resolve_fresh_agent_model(
        &svc.agent_registry,
        &agent_id,
        None,
        None,
    )
    .and_then(|model| {
        svc.agent_registry
            .with_provider(&agent_id, |p| p.model_launch_args(&model))
    })
    .unwrap_or_default();
    let launch_task =
        crate::service::provider_launch_task(&svc.agent_registry, &agent_id, None, None, None);
    let params = crate::agent::provider::AgentLaunchParams {
        agent_name: None,
        task: launch_task,
        starter_prompt: Default::default(),
        args: if model_args.is_empty() {
            None
        } else {
            Some(model_args)
        },
        skip_permissions,
        session_id: None,
        structured_prompt_server: None,
    };
    svc.agent_registry
        .with_provider(&agent_id, |p| {
            crate::agent::stub_launch::apply(&agent_id, p.build_launch_command(&params))
        })
        .ok_or_else(|| format!("Unknown agent provider: {}", agent_id))
}

/// The one atomic spawn command: create + cwd + agent_type + activate + focus a tab.
#[tauri::command]
pub fn mux_spawn_tab(
    svc: State<'_, ServiceState>,
    params: crate::service::transport::AtomicSpawnParams,
) -> Result<crate::service::transport::AtomicSpawn, String> {
    svc.spawn_tab_atomic(params)
}

/// Paired orchestration creation.
#[tauri::command]
pub async fn mux_create_orchestration_pair(
    svc: State<'_, ServiceState>,
    params: crate::service::OrchestrationPairParams,
) -> Result<crate::service::OrchestrationPairResult, String> {
    svc.create_orchestration_pair(params).await
}

/// Set a tab's orchestration role.
#[tauri::command]
pub fn mux_set_tab_role(
    svc: State<'_, ServiceState>,
    tab_id: String,
    role: String,
    parent_tab_id: Option<String>,
) -> Result<(), String> {
    svc.set_tab_role(&tab_id, &role, parent_tab_id.as_deref(), None)
}

/// Clear a tab's orchestration role (also orphans children).
#[tauri::command]
pub fn mux_clear_tab_role(svc: State<'_, ServiceState>, tab_id: String) -> Result<(), String> {
    svc.clear_tab_role(&tab_id)
}

/// Promote a tab to Orchestrator and inject the onboarding cheat-sheet.
#[tauri::command]
pub fn mux_make_orchestrator(svc: State<'_, ServiceState>, tab_id: String) -> Result<(), String> {
    svc.make_orchestrator(&tab_id)
}

#[tauri::command]
pub fn mux_make_planner(
    svc: State<'_, ServiceState>,
    tab_id: String,
    plan_id: String,
) -> Result<(), String> {
    svc.transition_plan_planner(
        &plan_id,
        Some(crate::plan::PlannerRef::Tab {
            tab_id,
            session_id: None,
            title: None,
        }),
    )?;
    Ok(())
}

/// Toggle whether a tab can spawn child agents.
#[tauri::command]
pub fn mux_set_can_spawn(
    svc: State<'_, ServiceState>,
    tab_id: String,
    can_spawn: bool,
) -> Result<(), String> {
    svc.set_can_spawn(&tab_id, can_spawn)
}

/// Respond to a pending prompt.
#[tauri::command]
pub fn mux_respond_prompt(
    svc: State<'_, ServiceState>,
    tab_id: String,
    value: String,
) -> Result<(), String> {
    svc.respond_prompt(&tab_id, &value)
}

/// Execute a batch of commands via the service layer.
#[tauri::command]
pub async fn mux_batch(
    svc: State<'_, ServiceState>,
    commands: Vec<BatchOp>,
) -> Result<Vec<serde_json::Value>, String> {
    Ok(svc.batch(commands).await)
}
