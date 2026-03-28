use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;
use tauri::State;

use super::state::{MultiplexerSnapshot, MultiplexerState, WorkspaceInfo};
use super::types::Orientation;
use crate::service::{BatchOp, ServiceState};

/// Shared multiplexer state, thread-safe.
pub type MuxState = Arc<Mutex<MultiplexerState>>;

// ── Response types ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct SplitResult {
    pub new_pane_id: String,
    pub new_pty_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TabInfo {
    pub id: String,
    pub title: String,
    pub pane_id: String,
}

// ── Commands ─────────────────────────────────────────────────────────
//
// All state-mutating commands delegate to ServiceContext for consistent
// behavior (PTY cleanup, event emission). Read-only commands access
// MuxState directly since they don't need side-effects.

/// Returns the full multiplexer state for frontend hydration (read-only).
#[tauri::command]
pub fn mux_get_state(state: State<'_, MuxState>) -> Result<MultiplexerSnapshot, String> {
    Ok(state.lock().snapshot())
}

/// Create a new workspace.
#[tauri::command]
pub fn mux_create_workspace(
    svc: State<'_, ServiceState>,
    name: String,
) -> Result<WorkspaceInfo, String> {
    Ok(svc.create_workspace(name))
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
) -> Result<TabInfo, String> {
    let (tab_id, pane_id) = svc.create_tab(&workspace_id)?;
    // User-created tabs are Orchestrators by default (can spawn agents)
    let _ = svc.set_pane_role(&pane_id, "Orchestrator", None, Some(true));
    // Read tab title from state
    let title = svc.mux.lock()
        .get_workspace(&workspace_id)
        .and_then(|ws| ws.tabs.iter().find(|t| t.id == tab_id))
        .map(|t| t.title.clone())
        .unwrap_or_else(|| "Tab".to_string());
    Ok(TabInfo { id: tab_id, title, pane_id })
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

/// Split a pane: creates a new pane (frontend will spawn PTY).
#[tauri::command]
pub fn mux_split_pane(
    svc: State<'_, ServiceState>,
    pane_id: String,
    orientation: Orientation,
    _cols: u16,
    _rows: u16,
) -> Result<SplitResult, String> {
    let new_pane_id = svc.split_pane(&pane_id, orientation)?;
    Ok(SplitResult {
        new_pane_id,
        new_pty_id: String::new(), // Frontend will fill this
    })
}

/// Close a pane and kill its PTY (cascade-closes children).
#[tauri::command]
pub fn mux_close_pane(
    svc: State<'_, ServiceState>,
    pane_id: String,
) -> Result<(), String> {
    svc.close_pane(&pane_id)
}

/// Set the focused pane.
#[tauri::command]
pub fn mux_set_focused_pane(
    svc: State<'_, ServiceState>,
    pane_id: String,
) -> Result<(), String> {
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

/// Generic agent session detection — delegates to provider from registry.
pub fn detect_agent_session_id(
    registry: &crate::agent::registry::AgentRegistry,
    agent_type: &str,
    cwd: &str,
    exclude: Option<Vec<String>>,
    max_age_secs: Option<u64>,
) -> Result<Option<String>, String> {
    let provider = registry.get(agent_type)
        .ok_or_else(|| format!("Unknown agent type: {}", agent_type))?;
    let excluded = exclude.unwrap_or_default();
    let max_age = max_age_secs.map(std::time::Duration::from_secs);
    Ok(provider.detect_session_id(cwd, &excluded, max_age))
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
pub fn mux_clear_pane_role(
    svc: State<'_, ServiceState>,
    pane_id: String,
) -> Result<(), String> {
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
