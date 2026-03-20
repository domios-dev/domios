use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, State};

use super::state::{MultiplexerSnapshot, MultiplexerState, WorkspaceInfo};
use super::types::Orientation;
use crate::notification::NotifState;
use crate::PtyState;

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

/// Returns the full multiplexer state for frontend hydration.
#[tauri::command]
pub fn mux_get_state(state: State<'_, MuxState>) -> Result<MultiplexerSnapshot, String> {
    let mux = state.lock();
    Ok(mux.snapshot())
}

/// Create a new workspace.
#[tauri::command]
pub fn mux_create_workspace(
    state: State<'_, MuxState>,
    name: String,
) -> Result<WorkspaceInfo, String> {
    let mut mux = state.lock();
    Ok(mux.create_workspace(name))
}

/// Switch the active workspace.
#[tauri::command]
pub fn mux_set_active_workspace(
    state: State<'_, MuxState>,
    workspace_id: String,
) -> Result<(), String> {
    let mut mux = state.lock();
    mux.set_active_workspace(&workspace_id)
}

/// Close a workspace and kill all its PTYs.
#[tauri::command]
pub fn mux_close_workspace(
    state: State<'_, MuxState>,
    pty_state: State<'_, PtyState>,
    workspace_id: String,
) -> Result<(), String> {
    let pty_ids = {
        let mut mux = state.lock();
        mux.close_workspace(&workspace_id)?
    };

    kill_ptys(&pty_state, &pty_ids);
    Ok(())
}

/// Rename a workspace.
#[tauri::command]
pub fn mux_rename_workspace(
    state: State<'_, MuxState>,
    workspace_id: String,
    name: String,
) -> Result<(), String> {
    let mut mux = state.lock();
    mux.rename_workspace(&workspace_id, &name)
}

/// Move a tab from one workspace to another.
#[tauri::command]
pub fn mux_move_tab(
    state: State<'_, MuxState>,
    tab_id: String,
    from_workspace_id: String,
    to_workspace_id: String,
) -> Result<(), String> {
    let mut mux = state.lock();
    mux.move_tab(&tab_id, &from_workspace_id, &to_workspace_id)
}

/// Create a new tab in a workspace.
#[tauri::command]
pub fn mux_create_tab(
    state: State<'_, MuxState>,
    workspace_id: String,
) -> Result<TabInfo, String> {
    let mut mux = state.lock();
    let (tab_id, pane_id) = mux.create_tab(&workspace_id)?;

    let title = mux
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
    state: State<'_, MuxState>,
    pty_state: State<'_, PtyState>,
    workspace_id: String,
    tab_id: String,
) -> Result<(), String> {
    let pty_ids = {
        let mut mux = state.lock();
        mux.close_tab(&workspace_id, &tab_id)?
    };

    kill_ptys(&pty_state, &pty_ids);
    Ok(())
}

/// Switch the active tab in a workspace.
#[tauri::command]
pub fn mux_set_active_tab(
    state: State<'_, MuxState>,
    workspace_id: String,
    tab_id: String,
) -> Result<(), String> {
    let mut mux = state.lock();
    mux.set_active_tab(&workspace_id, &tab_id)
}

/// Split a pane: creates a new pane and spawns a PTY for it.
#[tauri::command]
pub fn mux_split_pane(
    state: State<'_, MuxState>,
    _pty_state: State<'_, PtyState>,
    _notif_state: State<'_, NotifState>,
    _app: AppHandle,
    pane_id: String,
    orientation: Orientation,
    _cols: u16,
    _rows: u16,
) -> Result<SplitResult, String> {
    // Create new pane in the split tree (no PTY — frontend will spawn it)
    let new_pane_id = {
        let mut mux = state.lock();
        mux.split_pane(&pane_id, orientation)?
    };

    Ok(SplitResult {
        new_pane_id,
        new_pty_id: String::new(), // Frontend will fill this
    })
}

/// Close a pane and kill its PTY.
#[tauri::command]
pub fn mux_close_pane(
    state: State<'_, MuxState>,
    pty_state: State<'_, PtyState>,
    pane_id: String,
) -> Result<(), String> {
    let pty_id = {
        let mut mux = state.lock();
        mux.close_pane(&pane_id)?
    };

    if let Some(id) = pty_id {
        let mut manager = pty_state.lock();
        let _ = manager.kill(&id);
    }

    Ok(())
}

/// Set the focused pane.
#[tauri::command]
pub fn mux_set_focused_pane(
    state: State<'_, MuxState>,
    pane_id: String,
) -> Result<(), String> {
    let mut mux = state.lock();
    mux.set_focused_pane(&pane_id)
}

#[tauri::command]
pub fn mux_link_pty(
    state: State<'_, MuxState>,
    pane_id: String,
    pty_id: String,
) -> Result<(), String> {
    let mut mux = state.lock();
    mux.link_pty(&pane_id, pty_id);
    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Kill a list of PTYs by their IDs. Ignores errors for already-exited PTYs.
fn kill_ptys(pty_state: &State<'_, PtyState>, pty_ids: &[String]) {
    let mut manager = pty_state.lock();
    for pty_id in pty_ids {
        let _ = manager.kill(pty_id);
    }
}
