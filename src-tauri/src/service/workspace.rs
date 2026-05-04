//! Workspace & tab operations.

use crate::multiplexer::state::WorkspaceInfo;
use super::ServiceContext;

impl ServiceContext {
    pub fn list_workspaces(&self) -> Vec<WorkspaceInfo> {
        self.mux.lock().list_workspaces()
    }

    pub fn create_workspace(&self, name: String, cwd: Option<String>) -> WorkspaceInfo {
        // Dedup: if the user is opening a folder that matches an existing
        // workspace's cwd (after normalization — trailing slashes, case on
        // Windows, symlinks all collapse), reuse it and spawn a new tab
        // instead of creating a duplicate workspace. Keeps CLI/REST/Tauri
        // callers consistent without relying on frontend pre-checks.
        if let Some(cwd_str) = cwd.as_deref() {
            let existing = self.mux.lock().find_workspace_info_by_cwd(cwd_str);
            if let Some(ws) = existing {
                let _ = self.mux.lock().set_active_workspace(&ws.id);
                // New tab inside the existing workspace, activated. Don't
                // auto-spawn a second agent — the existing workspace already
                // has one running from its original creation.
                let _ = self.create_tab_with_activate(&ws.id, true);
                log::info!(
                    "[workspace] reused existing workspace '{}' ({}) for cwd={}",
                    ws.name,
                    ws.id,
                    cwd_str
                );
                return ws;
            }
        }

        let ws = self.mux.lock().create_workspace(name.clone(), cwd.clone());
        // Switch to the new workspace immediately
        let _ = self.mux.lock().set_active_workspace(&ws.id);
        self.emit_state_changed();

        // Auto-spawn agent in the new workspace
        let ws_id = ws.id.clone();
        let svc = self.clone();
        std::thread::spawn(move || {
            let params = super::orchestrator::SpawnAgentParams {
                workspace_id: Some(ws_id),
                cwd,
                title: Some(name),
                skip_permissions: Some(true),
                agent: None,
                agent_name: None,
                args: None,
                role: None,
                parent_pane_id: None,
                can_spawn: None,
                task: None,
                force_tab: None,
                report_target_pane_id: None,
                spawned_by_pane_id: None,
                model: None,
            };
            if let Err(e) = svc.spawn_agent_sync(params) {
                log::error!("[workspace] Failed to auto-spawn agent: {}", e);
            }
        });

        ws
    }

    pub fn focus_workspace(&self, id: &str) -> Result<(), String> {
        self.mux.lock().set_active_workspace(id)?;
        self.emit_state_changed();
        Ok(())
    }

    pub fn rename_workspace(&self, id: &str, name: &str) -> Result<(), String> {
        self.mux.lock().rename_workspace(id, name)?;
        self.emit_state_changed();
        Ok(())
    }

    pub fn close_workspace(&self, id: &str) -> Result<(), String> {
        let pre_focus = self.mux.lock().get_focused_pane_id().map(str::to_string);
        let pty_ids = self.mux.lock().close_workspace(id)?;
        let post_focus = self.mux.lock().get_focused_pane_id().map(str::to_string);
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
            .lock()
            .move_tab(tab_id, from_workspace_id, to_workspace_id)?;
        self.emit_state_changed();
        Ok(())
    }

    pub fn create_tab(&self, workspace_id: &str) -> Result<(String, String), String> {
        self.create_tab_with_activate(workspace_id, true)
    }

    pub fn create_tab_with_activate(&self, workspace_id: &str, activate: bool) -> Result<(String, String), String> {
        let result = {
            let mut mux = self.mux.lock();
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
        let pre_focus = self.mux.lock().get_focused_pane_id().map(str::to_string);
        let pty_ids = self.mux.lock().close_tab(workspace_id, tab_id)?;
        let post_focus = self.mux.lock().get_focused_pane_id().map(str::to_string);
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
        self.mux.lock().set_active_tab(workspace_id, tab_id)?;
        self.emit_state_changed();
        Ok(())
    }
}
