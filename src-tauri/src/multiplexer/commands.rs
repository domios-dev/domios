use std::sync::Arc;

use parking_lot::RwLock;
use serde::Serialize;
use tauri::State;

use super::state::{MultiplexerSnapshot, MultiplexerState, WorkspaceInfo};
use crate::service::{BatchOp, ServiceState};

/// Shared multiplexer state, thread-safe. RwLock allows concurrent reads from
/// multiple PTY output handlers; writers still get exclusive access.
pub type MuxState = Arc<RwLock<MultiplexerState>>;

// ── Response types ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct TabInfo {
    pub id: String,
    pub title: String,
    pub pane_id: String,
}

// Mutating commands route through ServiceContext for PTY cleanup/event emission.
// Read-only commands read MuxState directly since they have no side effects.

/// Returns the full multiplexer state for frontend hydration (read-only).
#[tauri::command]
pub fn mux_get_state(state: State<'_, MuxState>) -> Result<MultiplexerSnapshot, String> {
    Ok(state.write().snapshot())
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

/// Create a new tab in a workspace.
#[tauri::command]
pub fn mux_create_tab(
    svc: State<'_, ServiceState>,
    workspace_id: String,
    eager: Option<bool>,
) -> Result<TabInfo, String> {
    let (tab_id, pane_id) = svc.create_tab(&workspace_id)?;
    // User-created tabs are Orchestrators by default (can spawn agents)
    let _ = svc.set_pane_role(&pane_id, "Orchestrator", None, Some(true));
    if eager.unwrap_or(false) {
        svc.ensure_pane_pty_for_send(&pane_id)?;
    }
    // Read tab title from state
    let title = svc
        .mux
        .write()
        .get_workspace(&workspace_id)
        .and_then(|ws| ws.tabs.iter().find(|t| t.id == tab_id))
        .map(|t| t.title.clone())
        .unwrap_or_else(|| "Tab".to_string());
    Ok(TabInfo {
        id: tab_id,
        title,
        pane_id,
    })
}

/// Close a tab and kill its PTYs.
#[tauri::command]
pub fn mux_close_tab(
    svc: State<'_, ServiceState>,
    workspace_id: String,
    tab_id: String,
) -> Result<(), String> {
    svc.close_tab(&workspace_id, &tab_id)
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

/// Atomically switch active workspace and active tab in a single lock
/// acquisition, emitting one mux-state-changed instead of two.
#[tauri::command]
pub fn mux_set_active_tab_and_workspace(
    svc: State<'_, ServiceState>,
    workspace_id: String,
    tab_id: String,
) -> Result<(), String> {
    svc.set_active_tab_and_workspace(&workspace_id, &tab_id)
}

/// Rename a tab. Only path that changes a tab's title — pane renames don't propagate.
#[tauri::command]
pub fn mux_rename_tab(
    svc: State<'_, ServiceState>,
    workspace_id: String,
    tab_id: String,
    title: String,
) -> Result<(), String> {
    svc.rename_tab(&workspace_id, &tab_id, &title)
}

/// Close a pane and kill its PTY (cascade-closes children).
#[tauri::command]
pub fn mux_close_pane(svc: State<'_, ServiceState>, pane_id: String) -> Result<(), String> {
    svc.close_pane(&pane_id)
}

/// Set the focused pane.
#[tauri::command]
pub fn mux_set_focused_pane(svc: State<'_, ServiceState>, pane_id: String) -> Result<(), String> {
    svc.focus_pane(&pane_id)
}

/// Link a PTY to a pane.
#[tauri::command]
pub fn mux_link_pty(
    svc: State<'_, ServiceState>,
    pane_id: String,
    pty_id: String,
) -> Result<(), String> {
    svc.link_pty(&pane_id, pty_id);
    Ok(())
}

/// Rename a pane (User-level — highest priority, blocks auto-rename).
#[tauri::command]
pub fn mux_rename_pane(
    svc: State<'_, ServiceState>,
    pane_id: String,
    title: String,
) -> Result<(), String> {
    svc.rename_pane(&pane_id, &title)
}

/// Update the working directory of a pane (called from OSC 7 handler).
#[tauri::command]
pub fn mux_update_pane_cwd(
    svc: State<'_, ServiceState>,
    pane_id: String,
    cwd: String,
) -> Result<(), String> {
    svc.update_pane_cwd(&pane_id, cwd);
    Ok(())
}

/// Update the agent type of a pane (called when Claude Code is detected).
#[tauri::command]
pub fn mux_set_pane_agent_type(
    svc: State<'_, ServiceState>,
    pane_id: String,
    agent_type: Option<String>,
) -> Result<(), String> {
    svc.set_pane_agent_type(&pane_id, agent_type);
    Ok(())
}

/// Release the `agent_type_locked` flag on a pane without clearing `agent_type`.
/// Lets the user disconnect an agent identity manually and reopen the slot for
/// input-parsed re-detection.
#[tauri::command]
pub fn mux_unlock_pane_agent_type(
    svc: State<'_, ServiceState>,
    pane_id: String,
) -> Result<(), String> {
    svc.unlock_pane_agent_type(&pane_id);
    Ok(())
}

/// Set chat view state on a pane (persists across restart).
#[tauri::command]
pub fn mux_set_chat_view(
    svc: State<'_, ServiceState>,
    pane_id: String,
    enabled: bool,
) -> Result<(), String> {
    svc.set_chat_view(&pane_id, enabled);
    Ok(())
}

/// Detect agent session IDs for a given working directory.
/// Uses AgentRegistry to delegate to the appropriate provider.
/// Keeps the old Tauri command name for frontend compatibility.
#[tauri::command]
pub fn detect_claude_session_id(
    svc: State<'_, ServiceState>,
    cwd: String,
    exclude: Option<Vec<String>>,
    max_age_secs: Option<u64>,
) -> Result<Option<String>, String> {
    detect_agent_session_id(&svc.agent_registry, "claude", &cwd, exclude, max_age_secs)
}

/// Provider-agnostic version of `detect_claude_session_id`. Frontend calls
/// this for codex / opencode / future providers — the registry dispatches
/// to the right `AgentProvider::detect_session_id` impl, so adding a new
/// plugin Just Works without touching the tauri command surface.
#[tauri::command]
pub fn detect_pane_session_id(
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

/// Generic agent session detection — delegates to provider from registry.
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

/// Store a detected agent session ID on a pane.
/// Keeps the old Tauri command name `mux_set_claude_session_id` for frontend compatibility.
#[tauri::command]
pub fn mux_set_claude_session_id(
    svc: State<'_, ServiceState>,
    pane_id: String,
    session_id: Option<String>,
) -> Result<(), String> {
    svc.set_agent_session_id(&pane_id, session_id);
    Ok(())
}

/// Build the shell launch command for a given agent provider.
/// Used by frontend to spawn agent tabs without hardcoding CLI names.
#[tauri::command]
pub fn build_agent_launch_command(
    svc: State<'_, ServiceState>,
    agent_id: String,
    skip_permissions: bool,
) -> Result<String, String> {
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
    let params = crate::agent::provider::AgentLaunchParams {
        agent_name: None,
        task: None,
        args: if model_args.is_empty() {
            None
        } else {
            Some(model_args)
        },
        skip_permissions,
        session_id: None,
    };
    svc.agent_registry
        .with_provider(&agent_id, |p| p.build_launch_command(&params))
        .ok_or_else(|| format!("Unknown agent provider: {}", agent_id))
}

// ── Agent Hierarchy Commands ─────────────────────────────────────────

/// Set a pane's orchestration role.
#[tauri::command]
pub fn mux_set_pane_role(
    svc: State<'_, ServiceState>,
    pane_id: String,
    role: String,
    parent_pane_id: Option<String>,
) -> Result<(), String> {
    svc.set_pane_role(&pane_id, &role, parent_pane_id.as_deref(), None)
}

/// Clear a pane's orchestration role (also orphans children).
#[tauri::command]
pub fn mux_clear_pane_role(svc: State<'_, ServiceState>, pane_id: String) -> Result<(), String> {
    svc.clear_pane_role(&pane_id)
}

/// Toggle whether a pane can spawn child agents.
#[tauri::command]
pub fn mux_set_can_spawn(
    svc: State<'_, ServiceState>,
    pane_id: String,
    can_spawn: bool,
) -> Result<(), String> {
    svc.set_can_spawn(&pane_id, can_spawn)
}

/// Respond to a pending prompt — sends the value to PTY and clears the prompt.
#[tauri::command]
pub fn mux_respond_prompt(
    svc: State<'_, ServiceState>,
    pane_id: String,
    value: String,
) -> Result<(), String> {
    svc.respond_prompt(&pane_id, &value)
}

// ── Batch (unified via service layer) ────────────────────────────────

/// Execute a batch of commands via the service layer.
/// Same semantics as POST /api/batch — supports $ref resolution.
#[tauri::command]
pub async fn mux_batch(
    svc: State<'_, ServiceState>,
    commands: Vec<BatchOp>,
) -> Result<Vec<serde_json::Value>, String> {
    Ok(svc.batch(commands).await)
}
