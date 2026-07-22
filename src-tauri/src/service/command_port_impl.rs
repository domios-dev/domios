use crate::dispatch::CommandPort;
use crate::multiplexer::state::{MultiplexerSnapshot, WorkspaceInfo};
use crate::multiplexer::types::ClosedPane;
use crate::service::ServiceContext;

impl CommandPort for ServiceContext {
    fn mark_user_activity(&self) {
        self.user_last_activity.mark();
    }

    fn mux_snapshot(&self) -> MultiplexerSnapshot {
        self.mux.read().snapshot()
    }

    fn send_text(&self, pane_id: &str, text: &str, enter: bool) -> Result<(), String> {
        // Fire-and-forget so interactive sends never wait for terminal evidence.
        self.send_text_detached(pane_id, text, enter);
        Ok(())
    }

    fn read_pane_output(
        &self,
        pane_id: &str,
        offset: Option<usize>,
        max_bytes: Option<usize>,
        strip_ansi: bool,
        full: bool,
    ) -> Result<serde_json::Value, String> {
        self.pane_transport(pane_id)?
            .read_output(offset, max_bytes, strip_ansi, full)
    }

    fn read_pane_screen(
        &self,
        pane_id: &str,
        formatted: bool,
    ) -> Result<serde_json::Value, String> {
        self.pane_transport(pane_id)?.read_screen(formatted)
    }

    fn close_pane_guarded(&self, caller: Option<&str>, pane_id: &str) -> Result<(), String> {
        ServiceContext::close_pane_guarded(self, caller, pane_id)
    }

    fn focus_pane(&self, pane_id: &str) -> Result<(), String> {
        ServiceContext::focus_pane(self, pane_id)
    }

    fn rename_pane(&self, pane_id: &str, title: &str) -> Result<(), String> {
        ServiceContext::rename_pane(self, pane_id, title)
    }

    fn set_pane_description(&self, pane_id: &str, desc: &str) -> Result<(), String> {
        ServiceContext::set_pane_description(self, pane_id, desc)
    }

    fn create_workspace(&self, name: String, cwd: Option<String>, activate: bool) -> WorkspaceInfo {
        ServiceContext::create_workspace(self, name, cwd, activate)
    }

    fn rename_workspace(&self, ws_id: &str, name: &str) -> Result<(), String> {
        ServiceContext::rename_workspace(self, ws_id, name)
    }

    fn close_workspace_guarded(&self, caller: Option<&str>, ws_id: &str) -> Result<(), String> {
        ServiceContext::close_workspace_guarded(self, caller, ws_id)
    }

    fn list_workspaces(&self) -> Vec<WorkspaceInfo> {
        ServiceContext::list_workspaces(self)
    }

    fn list_recently_closed(&self, ws_id: &str) -> Vec<ClosedPane> {
        ServiceContext::list_recently_closed(self, ws_id)
    }

    fn reopen_closed_pane(
        &self,
        ws_id: &str,
        index: Option<usize>,
    ) -> Result<(String, String), String> {
        ServiceContext::reopen_closed_pane(self, ws_id, index)
    }

    fn reopen_closed_group(&self, group_id: &str) -> Result<(String, String, String), String> {
        ServiceContext::reopen_closed_group(self, group_id)
    }

    fn resize_pane(&self, pane_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        ServiceContext::resize_pane(self, pane_id, cols, rows)
    }

    fn kill_pane_pty(&self, pane_id: &str) -> Result<(), String> {
        ServiceContext::kill_pane_pty(self, pane_id)
    }

    fn list_agent_providers(&self) -> Vec<serde_json::Value> {
        crate::api::agent_routes::list_agent_provider_values(&self.provider_registry)
    }
}
