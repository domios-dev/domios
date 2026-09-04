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
}
