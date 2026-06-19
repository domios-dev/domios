//! Workspace & tab operations.

use super::ServiceContext;
use crate::multiplexer::state::WorkspaceInfo;
use crate::multiplexer::types::ClosedPane;

impl ServiceContext {
    pub fn list_workspaces(&self) -> Vec<WorkspaceInfo> {
        self.mux.read().list_workspaces()
    }

    pub fn create_workspace(
        &self,
        name: String,
        cwd: Option<String>,
        activate: bool,
    ) -> WorkspaceInfo {
        if let Some(cwd_str) = cwd.as_deref() {
            let existing = self.mux.write().find_workspace_info_by_cwd(cwd_str);
            if let Some(ws) = existing {
                if activate {
                    let _ = self.mux.write().set_active_workspace(&ws.id);
                }
                log::info!(
                    "[workspace] reused existing workspace '{}' ({}) for cwd={}",
                    ws.name,
                    ws.id,
                    cwd_str
                );
                return WorkspaceInfo {
                    existing: Some(true),
                    ..ws
                };
            }
        }

        let ws = self.mux.write().create_workspace(name, cwd);
        if activate {
            let _ = self.mux.write().set_active_workspace(&ws.id);
        }
        self.emit_state_changed();
        ws
    }

    pub fn focus_workspace(&self, id: &str) -> Result<(), String> {
        self.mux.write().set_active_workspace(id)?;
        self.emit_state_changed();
        Ok(())
    }

    pub fn rename_workspace(&self, id: &str, name: &str) -> Result<(), String> {
        self.mux.write().rename_workspace(id, name)?;
        self.emit_state_changed();
        Ok(())
    }

    pub fn close_workspace(&self, id: &str) -> Result<(), String> {
        let pre_focus = self.mux.read().get_focused_pane_id().map(str::to_string);
        let pty_ids = self.mux.write().close_workspace(id)?;
        let post_focus = self.mux.read().get_focused_pane_id().map(str::to_string);
        self.kill_ptys(&pty_ids);
        self.emit_state_changed();
        if pre_focus != post_focus {
            if let Some(new) = post_focus {
                self.emit_pane_focus_changed(&new);
            }
        }
        Ok(())
    }

    pub fn move_tab(
        &self,
        tab_id: &str,
        from_workspace_id: &str,
        to_workspace_id: &str,
    ) -> Result<(), String> {
        self.mux
            .write()
            .move_tab(tab_id, from_workspace_id, to_workspace_id)?;
        self.emit_state_changed();
        Ok(())
    }

    pub fn create_tab(&self, workspace_id: &str) -> Result<(String, String), String> {
        self.create_tab_with_activate(workspace_id, true)
    }

    pub fn create_tab_with_activate(
        &self,
        workspace_id: &str,
        activate: bool,
    ) -> Result<(String, String), String> {
        let result = {
            let mut mux = self.mux.write();
            let (tab_id, pane_id) = mux.create_tab(workspace_id)?;
            if activate {
                let _ = mux.set_active_tab(workspace_id, &tab_id);
            }
            (tab_id, pane_id)
        };
        self.emit_state_changed();
        Ok(result)
    }

    pub fn close_tab(&self, workspace_id: &str, tab_id: &str) -> Result<(), String> {
        let pre_focus = self.mux.read().get_focused_pane_id().map(str::to_string);
        let pty_ids = self.mux.write().close_tab(workspace_id, tab_id)?;
        let post_focus = self.mux.read().get_focused_pane_id().map(str::to_string);
        self.kill_ptys(&pty_ids);
        self.emit_state_changed();
        if pre_focus != post_focus {
            if let Some(new) = post_focus {
                self.emit_pane_focus_changed(&new);
            }
        }
        Ok(())
    }

    pub fn set_active_tab(&self, workspace_id: &str, tab_id: &str) -> Result<(), String> {
        self.mux.write().set_active_tab(workspace_id, tab_id)?;
        self.emit_state_changed();
        Ok(())
    }

    /// Atomically set active workspace and active tab in one lock acquisition,
    /// emitting a single mux-state-changed instead of two sequential ones.
    pub fn set_active_tab_and_workspace(
        &self,
        workspace_id: &str,
        tab_id: &str,
    ) -> Result<(), String> {
        {
            let mut mux = self.mux.write();
            mux.set_active_workspace(workspace_id)?;
            mux.set_active_tab(workspace_id, tab_id)?;
        }
        self.emit_state_changed();
        Ok(())
    }

    pub fn rename_tab(&self, workspace_id: &str, tab_id: &str, title: &str) -> Result<(), String> {
        self.mux.write().rename_tab(workspace_id, tab_id, title)?;
        self.emit_state_changed();
        Ok(())
    }

    /// Snapshot of a workspace's recently-closed ring (newest first).
    pub fn list_recently_closed(&self, workspace_id: &str) -> Vec<ClosedPane> {
        self.mux.read().list_recently_closed(workspace_id)
    }

    /// Reopen a recently-closed pane (browser-style). `index` picks the ring
    /// entry (newest = 0, default). Re-materializes the pane with its original
    /// backing in a fresh, activated tab; the restore path then resumes a PTY
    /// agent or lazily reloads a chatBackend transcript. Returns (tab_id, pane_id).
    pub fn reopen_closed_pane(
        &self,
        workspace_id: &str,
        index: Option<usize>,
    ) -> Result<(String, String), String> {
        let (tab_id, pane_id) = {
            let mut mux = self.mux.write();
            let result = mux.reopen_closed_pane(workspace_id, index)?;
            let _ = mux.set_active_tab(workspace_id, &result.0);
            result
        };
        self.emit_state_changed();
        Ok((tab_id, pane_id))
    }
}
