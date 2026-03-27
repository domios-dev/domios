//! Pane lifecycle: split, close, rename, focus, browser.

use crate::multiplexer::types::Orientation;
use super::ServiceContext;

impl ServiceContext {
    pub fn split_pane(&self, pane_id: &str, orientation: Orientation) -> Result<String, String> {
        let new_pane_id = self.mux.lock().split_pane(pane_id, orientation)?;
        self.emit_state_changed();
        Ok(new_pane_id)
    }

    pub fn close_pane(&self, pane_id: &str) -> Result<(), String> {
        let pty_ids = self.mux.lock().close_pane(pane_id)?;
        self.kill_ptys(&pty_ids);
        self.emit_state_changed();
        Ok(())
    }

    pub fn rename_pane(&self, pane_id: &str, title: &str) -> Result<(), String> {
        self.mux.lock().rename_pane(pane_id, title)?;
        self.emit_pane_title_pinned(pane_id, title);
        self.emit_state_changed();
        Ok(())
    }

    pub fn focus_pane(&self, pane_id: &str) -> Result<(), String> {
        self.mux.lock().set_focused_pane(pane_id)?;
        self.emit_state_changed();
        Ok(())
    }

    pub fn open_browser(&self, url: String) -> Result<String, String> {
        let pane_id = self.mux.lock().open_browser(Some(url))?;
        self.emit_state_changed();
        Ok(pane_id)
    }

    pub fn link_pty(&self, pane_id: &str, pty_id: String) {
        self.mux.lock().link_pty(pane_id, pty_id);
    }

    pub fn update_pane_cwd(&self, pane_id: &str, cwd: String) {
        if let Some(pane) = self.mux.lock().get_pane_mut(pane_id) {
            pane.cwd = Some(cwd);
        }
    }

    pub fn set_pane_agent_type(&self, pane_id: &str, agent_type: Option<String>) {
        if let Some(pane) = self.mux.lock().get_pane_mut(pane_id) {
            pane.agent_type = agent_type;
        }
    }

    /// Set agent session ID on a pane. Kept as `set_agent_session_id`.
    pub fn set_agent_session_id(&self, pane_id: &str, session_id: Option<String>) {
        if let Some(pane) = self.mux.lock().get_pane_mut(pane_id) {
            pane.agent_session_id = session_id;
        }
    }

    /// Backward-compat alias for `set_agent_session_id`.
    pub fn set_claude_session_id(&self, pane_id: &str, session_id: Option<String>) {
        self.set_agent_session_id(pane_id, session_id);
    }
}
