//! Single owner of provisional-tab rollback: removal + authorized-reduction persist together.

use crate::multiplexer::types::TabCloseReason;

impl super::ServiceContext {
    /// Roll back a provisional spawn tab (no closed-history record) and persist:
    /// a concurrent/debounced save may already have captured it in last.json.
    pub(crate) fn rollback_provisional_tab(&self, ws_id: &str, tab_id: &str) {
        self.mux.write().rollback_spawn_tab(ws_id, tab_id);
        super::session_persist::persist_session_after_close(self);
    }

    /// Same guarantee for rollbacks that retire the provisional tab through
    /// the ordinary mux close (records closed history).
    pub(crate) fn rollback_provisional_tab_via_close(&self, tab_id: &str) {
        let _ = self
            .mux
            .write()
            .close_tab_with_reason(tab_id, TabCloseReason::SpawnRollback);
        super::session_persist::persist_session_after_close(self);
    }

    pub(crate) fn rollback_empty_agent_launch(&self, tab_id: &str, provider_id: &str) -> String {
        self.cancel_agent_startup_supervision(tab_id);
        if let Err(error) = self.close_tab_with_reason(tab_id, TabCloseReason::SpawnRollback) {
            log::warn!(
                "[spawn] tab={} provider={} rollback close failed: {}",
                tab_id,
                provider_id,
                error
            );
            let _ = self.finish_tab_close(tab_id);
            self.rollback_provisional_tab_via_close(tab_id);
            if self.mux.read().get_tab(tab_id).is_some() {
                if let Some(workspace_id) = self.mux.read().find_workspace_for_tab(tab_id) {
                    self.rollback_provisional_tab(&workspace_id, tab_id);
                }
            }
            self.emit_state_changed();
            if self.mux.read().get_tab(tab_id).is_some() {
                return format!(
                    "Agent provider '{}' did not resolve a launch command; cleanup failed for Tab {}: {}",
                    provider_id, tab_id, error
                );
            }
        }
        format!(
            "Agent provider '{}' did not resolve a launch command",
            provider_id
        )
    }
}
