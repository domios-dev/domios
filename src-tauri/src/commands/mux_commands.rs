use serde::Serialize;
use tauri::State;

use crate::multiplexer::state::{MultiplexerSnapshot, WorkspaceInfo};
use crate::service::{BatchOp, ServiceState, TeamMutationCaller};
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
pub(crate) async fn mux_get_state_from_service(
    svc: ServiceState,
) -> Result<MultiplexerSnapshot, String> {
    refuse_busy_snapshot_off_runtime(move || svc.try_snapshot_with_reconciled_status()).await
}

async fn refuse_busy_snapshot_off_runtime(
    try_snapshot: impl FnOnce() -> Option<MultiplexerSnapshot> + Send + 'static,
) -> Result<MultiplexerSnapshot, String> {
    tokio::task::spawn_blocking(move || {
        try_snapshot().ok_or_else(|| crate::service::SNAPSHOT_BUSY_ERROR.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn mux_get_state(svc: State<'_, ServiceState>) -> Result<MultiplexerSnapshot, String> {
    mux_get_state_from_service(svc.inner().clone()).await
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
    force: Option<bool>,
) -> Result<(), String> {
    svc.close_workspace_guarded(None, &workspace_id, force.unwrap_or(false))
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
mod snapshot_refusal_tests {
    use super::refuse_busy_snapshot_off_runtime;
    use crate::agent::registry::AgentRegistry;
    use crate::agent::status::TabStatusTracker;
    use crate::lock_hold::{TimedMutex, TimedRwLock};
    use crate::multiplexer::commands::MuxState;
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::{try_snapshot_with_reconciled_status, SNAPSHOT_BUSY_ERROR};
    use crate::AgentTrackerState;
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    // Drives the command's refusal path without a Tauri app: tao refuses to build
    // its EventLoop off the main thread on macOS, and test threads are not main.
    #[test]
    fn authoritative_command_refuses_without_blocking_runtime() {
        let (state, _) = MultiplexerState::new();
        let mux: MuxState = Arc::new(TimedRwLock::new("mux-command-refusal-test", state));
        let tracker: AgentTrackerState = Arc::new(TimedMutex::new(
            "mux-command-refusal-test",
            TabStatusTracker::new(),
        ));
        let _lifecycle_owner = mux.write();
        let worker_mux = mux.clone();
        let (done_tx, done_rx) = mpsc::channel();

        let worker = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let registry = AgentRegistry::new();
            let error = runtime
                .block_on(refuse_busy_snapshot_off_runtime(move || {
                    try_snapshot_with_reconciled_status(&worker_mux, &tracker, &registry)
                }))
                .expect_err("a lifecycle-owned mux must be refused");
            done_tx.send(error).unwrap();
        });

        assert_eq!(
            done_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("the Tauri command must not block its runtime"),
            SNAPSHOT_BUSY_ERROR
        );
        worker.join().unwrap();
    }
}

#[cfg(test)]
pub fn mux_rename_team_core(
    mux: &mut crate::multiplexer::state::MultiplexerState,
    team_id: String,
    name: String,
    caller_tab_id: Option<String>,
) -> Result<Team, String> {
    crate::service::rename_team_core(
        mux,
        &team_id,
        name,
        &TeamMutationCaller::from_tab(caller_tab_id.as_deref()),
    )
}

pub(crate) fn mux_rename_team_service_core(
    svc: &ServiceState,
    team_id: String,
    name: String,
    caller_tab_id: Option<String>,
) -> Result<Team, String> {
    svc.rename_team(
        &team_id,
        name,
        TeamMutationCaller::from_tab(caller_tab_id.as_deref()),
    )
}

/// Rename a Team through the service boundary so authorization and the state
/// change event remain identical for desktop and web callers.
#[tauri::command]
pub fn mux_rename_team(
    svc: State<'_, ServiceState>,
    team_id: String,
    name: String,
    caller_tab_id: Option<String>,
) -> Result<Team, String> {
    mux_rename_team_service_core(svc.inner(), team_id, name, caller_tab_id)
}

pub(crate) fn mux_close_team_service_core(
    svc: &ServiceState,
    team_id: String,
    caller_tab_id: Option<String>,
) -> Result<Team, String> {
    svc.close_team(&team_id, close_team_caller(caller_tab_id))
}

fn close_team_caller(caller_tab_id: Option<String>) -> TeamMutationCaller {
    caller_tab_id.map_or(TeamMutationCaller::Human, TeamMutationCaller::AgentTab)
}

#[tauri::command]
pub fn mux_close_team(
    svc: State<'_, ServiceState>,
    team_id: String,
    caller_tab_id: Option<String>,
) -> Result<Team, String> {
    mux_close_team_service_core(svc.inner(), team_id, caller_tab_id)
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
    // No caller is the bootstrap door: a human placing the first manager into a Team
    // whose members cannot spawn. The service refuses it for any non-anchoring role.
    svc.add_team_member(
        &team_id,
        caller_tab_id.as_deref(),
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

/// `caller_tab_id` is optional on purpose: a Team with no spawn-capable member
/// left has no tab to spend, and that is exactly when it needs an Orchestrator.
#[tauri::command]
pub async fn mux_add_team_member(
    svc: State<'_, ServiceState>,
    team_id: String,
    provider: String,
    role: String,
    caller_tab_id: Option<String>,
    title: Option<String>,
    task: Option<String>,
    model: Option<String>,
) -> Result<crate::service::AddTeamMemberResult, String> {
    mux_add_team_member_service_core(
        svc.inner(),
        team_id,
        provider,
        role,
        caller_tab_id.filter(|tab_id| !tab_id.trim().is_empty()),
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
        mux.set_tab_role_kind(
            &planner,
            AgentRole::Manager,
            Some("planner"),
            Some(&orchestrator),
        )
        .unwrap();
        let worker = mux.create_tab(&workspace_id).unwrap();
        mux.set_tab_role(&worker, AgentRole::Worker, Some(&orchestrator))
            .unwrap();
        (mux, team_id, orchestrator, planner, worker)
    }

    /// Renaming needs no spawner. The old gate demanded a caller tab, so a Team
    /// whose only manager could not spawn had no way to be renamed either — a
    /// requirement `rename_team_core` never imposed.
    #[test]
    fn mux_rename_team_accepts_a_human_caller() {
        let (mut mux, team_id, orchestrator, _planner, _worker) = team_members();
        let renamed = mux_rename_team_core(&mut mux, team_id.clone(), "Renamed".into(), None)
            .expect("a human rename carries no caller tab");
        assert_eq!(renamed.name.as_deref(), Some("Renamed"));
        let by_agent = mux_rename_team_core(
            &mut mux,
            team_id,
            "By the manager".into(),
            Some(orchestrator),
        )
        .expect("an agent caller still renames");
        assert_eq!(by_agent.name.as_deref(), Some("By the manager"));
    }

    #[test]
    fn desktop_close_is_human_while_explicit_callers_keep_agent_authority() {
        assert_eq!(close_team_caller(None), TeamMutationCaller::Human);
        assert_eq!(
            close_team_caller(Some("manager".into())),
            TeamMutationCaller::AgentTab("manager".into())
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
            &TeamMutationCaller::AgentTab(orchestrator.clone()),
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
) -> Result<String, String> {
    svc.require_agent_provider_ready(&agent_id)?;
    let permission_bypass = crate::service::permission_policy::configured_permission_bypass(
        &svc.agent_registry,
        &agent_id,
    )?
    .is_some();
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
        starter_prompt: crate::service::starter_prompt_for_role(None),
        args: if model_args.is_empty() {
            None
        } else {
            Some(model_args)
        },
        skip_permissions: permission_bypass,
        session_id: None,
        structured_prompt_server: None,
        launch_marker: None,
    };
    svc.agent_registry
        .with_provider(&agent_id, |p| {
            crate::agent::stub_launch::apply(&agent_id, p.build_launch_command(&params))
        })
        .ok_or_else(|| format!("Unknown agent provider: {}", agent_id))
}

/// The one atomic spawn command: create + cwd + agent_type + activate + focus a tab.
#[tauri::command]
pub async fn mux_spawn_tab(
    svc: State<'_, ServiceState>,
    params: crate::service::transport::AtomicSpawnParams,
) -> Result<crate::service::transport::AtomicSpawn, String> {
    let svc = svc.inner().clone();
    tokio::task::spawn_blocking(move || svc.spawn_tab_atomic(params))
        .await
        .map_err(|error| format!("atomic spawn task failed: {error}"))?
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
