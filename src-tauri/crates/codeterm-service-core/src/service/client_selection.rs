//! Each client owns its selection; the daemon keeps only the last one.
//!
//! The daemon's active workspace, active tab and focused tab mean "the last
//! selection any client made". They seed a client that has no selection of its
//! own and survive desk restore. Recording a client's selection emits neither a
//! focus event nor a mux snapshot, so no other client reads, wakes or follows
//! it. An explicit focus request (CLI, socket, agent) is the one exception and
//! carries its own event that every client honours.

use super::ServiceContext;
use crate::service::session_persist::{self, SessionPersistUrgency};

/// Explicit request to show a tab, as opposed to a client's own selection.
pub(crate) const TAB_FOCUS_REQUESTED_EVENT: &str = "tab-focus-requested";

impl ServiceContext {
    /// Record one client's selection without telling any other client.
    pub fn record_client_selection(
        &self,
        workspace_id: Option<&str>,
        tab_id: Option<&str>,
    ) -> Result<(), String> {
        let tab_id = match tab_id {
            Some(raw) => Some(
                self.mux
                    .read()
                    .resolve_tab_id(raw)
                    .ok_or_else(|| format!("Tab not found: {raw}"))?,
            ),
            None => None,
        };
        let (switched, cwd, workspace_id) = {
            let mut mux = self.mux.write();
            let workspace_id = match (workspace_id, tab_id.as_deref()) {
                (Some(id), _) => Some(id.to_string()),
                (None, Some(tab)) => mux.find_workspace_for_tab(tab),
                (None, None) => return Err("selection names no workspace or tab".into()),
            };
            let switched = match workspace_id.as_deref() {
                Some(id) => {
                    let switched = mux.get_active_workspace().map(|w| w.id.as_str()) != Some(id);
                    mux.set_active_workspace(id)?;
                    if let Some(tab) = tab_id.as_deref() {
                        mux.set_active_tab(id, tab)?;
                    }
                    switched
                }
                None => false,
            };
            if let Some(tab) = tab_id.as_deref() {
                mux.set_focused_tab(tab)?;
            }
            let cwd = workspace_id
                .as_deref()
                .and_then(|id| mux.get_workspace(id))
                .and_then(|workspace| workspace.cwd.clone());
            (switched, cwd, workspace_id)
        };
        self.ext::<std::sync::Arc<crate::user_last_activity::UserLastActivity>>()
            .mark();
        if switched {
            self.record_product_activity();
        }
        if let Some(id) = workspace_id.as_deref() {
            self.refresh_workspace_cwd_state(id)?;
            self.ensure_workspace_plans_loaded(cwd.as_deref());
        }
        if let Some(tab) = tab_id.as_deref() {
            self.schedule_focused_tab_model_refresh();
            self.wake_opened_tab(tab);
        }
        let _ =
            session_persist::persist_session_after_mutation(self, SessionPersistUrgency::Debounced);
        Ok(())
    }

    /// An explicit request to show a tab: every client selects it.
    pub fn request_tab_focus(&self, tab_id: &str) -> Result<(), String> {
        self.focus_tab(tab_id)?;
        let (tab_id, workspace_id) = {
            let mux = self.mux.read();
            let tab_id = mux
                .resolve_tab_id(tab_id)
                .unwrap_or_else(|| tab_id.to_string());
            let workspace_id = mux.find_workspace_for_tab(&tab_id);
            (tab_id, workspace_id)
        };
        self.emit_focus_request(workspace_id.as_deref(), Some(&tab_id));
        Ok(())
    }

    /// An explicit request to show a workspace: every client opens it at its last selected tab.
    pub(super) fn request_workspace_focus(&self, workspace_id: &str) {
        let tab_id = self
            .mux
            .read()
            .get_workspace(workspace_id)
            .and_then(|workspace| workspace.active_tab_id.clone());
        self.emit_focus_request(Some(workspace_id), tab_id.as_deref());
    }

    fn emit_focus_request(&self, workspace_id: Option<&str>, tab_id: Option<&str>) {
        self.ui.emit(
            TAB_FOCUS_REQUESTED_EVENT,
            serde_json::json!({"tab_id": tab_id, "workspace_id": workspace_id}),
        );
    }
}

#[cfg(test)]
#[path = "client_selection/tests.rs"]
mod tests;
