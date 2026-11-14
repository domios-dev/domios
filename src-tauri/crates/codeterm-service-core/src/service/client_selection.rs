//! Each client owns its selection; the daemon keeps only the last one.
//!
//! The daemon's active workspace, active tab and focused tab mean "the last
//! selection any client made". They seed a client that has no selection of its
//! own and survive desk restore. Recording a client's selection emits neither a
//! focus event nor a mux snapshot, so no other client reads, wakes or follows
//! it. Only the owner moves a client's view, and only in the client they act in.
//!
//! A focus request from outside the clients (`tab focus`, `workspace focus`, the
//! socket or REST focus routes, a new Team's Orchestrator) moves no client and
//! leaves the last selection alone. Whatever its credential, an agent may have
//! issued it: QA agents drive an instance with the owner credential and can
//! type into the owner's shells. It highlights its tab in every client until a
//! client selects that tab.

use super::ServiceContext;
use crate::service::session_persist::{self, SessionPersistUrgency};
use crate::service::MuxEmitCause;

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
        let (switched, cwd, workspace_id, highlight_cleared) = {
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
            let highlight_cleared = tab_id
                .as_deref()
                .is_some_and(|tab| mux.set_tab_focus_requested(tab, false));
            let cwd = workspace_id
                .as_deref()
                .and_then(|id| mux.get_workspace(id))
                .and_then(|workspace| workspace.cwd.clone());
            (switched, cwd, workspace_id, highlight_cleared)
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
        if highlight_cleared {
            self.emit_state_changed(MuxEmitCause::TabMetadata);
        }
        let _ =
            session_persist::persist_session_after_mutation(self, SessionPersistUrgency::Debounced);
        Ok(())
    }

    /// A request to show a tab highlights it; no client moves.
    pub fn request_tab_focus(&self, tab_id: &str) -> Result<(), String> {
        let changed = {
            let mut mux = self.mux.write();
            let tab_id = mux
                .resolve_tab_id(tab_id)
                .ok_or_else(|| format!("Tab not found: {tab_id}"))?;
            mux.set_tab_focus_requested(&tab_id, true)
        };
        if changed {
            self.emit_state_changed(MuxEmitCause::TabMetadata);
        }
        Ok(())
    }

    /// A request to show a workspace highlights its last selected tab; no client moves.
    pub fn request_workspace_focus(&self, workspace_id: &str) -> Result<(), String> {
        let changed = {
            let mut mux = self.mux.write();
            let tab_id = mux
                .get_workspace(workspace_id)
                .ok_or_else(|| format!("Workspace not found: {workspace_id}"))?
                .active_tab_id
                .clone();
            tab_id.is_some_and(|tab_id| mux.set_tab_focus_requested(&tab_id, true))
        };
        if changed {
            self.emit_state_changed(MuxEmitCause::TabMetadata);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "client_selection/tests.rs"]
mod tests;
