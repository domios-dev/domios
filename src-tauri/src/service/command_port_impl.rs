use crate::dispatch::CommandPort;
use crate::multiplexer::state::{MultiplexerSnapshot, WorkspaceInfo};
use crate::multiplexer::types::ClosedTab;
use crate::service::ServiceContext;

impl CommandPort for ServiceContext {
    fn mark_user_activity(&self) {
        self.user_last_activity.mark();
    }

    fn mux_snapshot(&self) -> MultiplexerSnapshot {
        // Carries the same reconciled per-tab status the UI rail and `tab
        // status` report, so every surface agrees.
        self.snapshot_with_reconciled_status()
    }

    fn mux_snapshot_until(&self, deadline: std::time::Instant) -> Option<MultiplexerSnapshot> {
        crate::service::status_reconciliation::snapshot_with_reconciled_status_until(
            &self.mux,
            &self.agent_tracker,
            &self.agent_registry,
            deadline,
        )
    }

    fn send_text(&self, tab_id: &str, text: &str) -> Result<(), String> {
        // Fire-and-forget so interactive sends never wait for terminal evidence.
        self.send_text_detached(tab_id, text);
        Ok(())
    }

    fn read_tab_output(
        &self,
        tab_id: &str,
        offset: Option<usize>,
        max_bytes: Option<usize>,
        strip_ansi: bool,
        full: bool,
    ) -> Result<serde_json::Value, String> {
        self.tab_transport(tab_id)?
            .read_output(offset, max_bytes, strip_ansi, full)
    }

    fn read_tab_screen(&self, tab_id: &str, formatted: bool) -> Result<serde_json::Value, String> {
        self.tab_transport(tab_id)?.read_screen(formatted)
    }

    fn close_tab_guarded(
        &self,
        caller: Option<&str>,
        tab_id: &str,
        force: bool,
    ) -> Result<Vec<String>, String> {
        ServiceContext::close_tab_guarded(self, caller, tab_id, force)
    }

    fn focus_tab(&self, tab_id: &str) -> Result<(), String> {
        ServiceContext::focus_tab(self, tab_id)
    }

    fn rename_tab(&self, tab_id: &str, title: &str) -> Result<(), String> {
        ServiceContext::rename_tab(self, tab_id, title)
    }

    fn set_tab_description(&self, tab_id: &str, desc: &str) -> Result<(), String> {
        ServiceContext::set_tab_description(self, tab_id, desc)
    }

    fn create_workspace(&self, name: String, cwd: Option<String>, activate: bool) -> WorkspaceInfo {
        ServiceContext::create_workspace(self, name, cwd, activate)
    }

    fn create_provisioned_workspace(
        &self,
        name: String,
        activate: bool,
    ) -> Result<WorkspaceInfo, String> {
        ServiceContext::create_provisioned_workspace(self, name, activate)
    }

    fn rename_workspace(&self, ws_id: &str, name: &str) -> Result<(), String> {
        ServiceContext::rename_workspace(self, ws_id, name)
    }

    fn close_workspace_guarded(
        &self,
        caller: Option<&str>,
        ws_id: &str,
        force: bool,
    ) -> Result<(), String> {
        ServiceContext::close_workspace_guarded(self, caller, ws_id, force)
    }

    fn list_workspaces(&self) -> Vec<WorkspaceInfo> {
        ServiceContext::list_workspaces(self)
    }

    fn list_recently_closed(&self, ws_id: &str) -> Vec<ClosedTab> {
        ServiceContext::list_recently_closed(self, ws_id)
    }

    fn reopen_closed_tab(&self, ws_id: &str, index: Option<usize>) -> Result<String, String> {
        ServiceContext::reopen_closed_tab(self, ws_id, index)
    }

    fn reopen_closed_group(&self, group_id: &str) -> Result<(String, String), String> {
        ServiceContext::reopen_closed_group(self, group_id)
    }

    fn resize_tab(&self, tab_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        ServiceContext::resize_tab(self, tab_id, cols, rows)
    }

    fn kill_tab_pty(&self, tab_id: &str) -> Result<(), String> {
        ServiceContext::kill_tab_pty(self, tab_id)
    }

    fn list_agent_providers(&self) -> Vec<serde_json::Value> {
        crate::api::agent_routes::list_agent_provider_values(&self.provider_registry)
    }
}

#[cfg(test)]
mod tests {
    use crate::agent::registry::AgentRegistry;
    use crate::agent::status::TabStatusTracker;
    use crate::lock_hold::{TimedMutex, TimedRwLock};
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::status_reconciliation::snapshot_with_reconciled_status_until;
    use std::sync::Arc;
    use std::time::Instant;

    #[test]
    fn a_writer_past_the_deadline_is_an_observable_refusal() {
        let (state, _) = MultiplexerState::new();
        let mux = Arc::new(TimedRwLock::new("mux", state));
        let tracker = Arc::new(TimedMutex::new("agent_tracker", TabStatusTracker::new()));
        let registry = AgentRegistry::new();
        let lifecycle_owner = mux.write();

        assert!(
            snapshot_with_reconciled_status_until(&mux, &tracker, &registry, Instant::now())
                .is_none()
        );
        drop(lifecycle_owner);
        assert!(
            snapshot_with_reconciled_status_until(&mux, &tracker, &registry, Instant::now())
                .is_some()
        );
    }

    #[test]
    fn gui_launch_keeps_authenticated_tabs_responsive() {
        use crate::api::access_policy::check_access;
        use crate::mesh::types::AccessLevel;
        use axum::http::Method;

        let (state, _) = MultiplexerState::new();
        let mux = Arc::new(TimedRwLock::new("mux", state));
        let tracker = Arc::new(TimedMutex::new("agent_tracker", TabStatusTracker::new()));
        let registry = AgentRegistry::new();

        // GUI startup may still be waiting for frontend_ready. That gate owns
        // only startup work; it must not gate an authenticated tab read.
        let generation = crate::startup::arm_frontend_readiness();
        assert!(!crate::startup::wait_for_frontend_ready_until(
            generation,
            Instant::now()
        ));
        assert!(check_access(&AccessLevel::Full, &Method::GET, "/api/tabs"));
        assert!(
            snapshot_with_reconciled_status_until(&mux, &tracker, &registry, Instant::now())
                .is_some(),
            "an authenticated tab read remains available before the GUI readiness event"
        );
    }
}
