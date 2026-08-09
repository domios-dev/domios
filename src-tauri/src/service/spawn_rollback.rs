//! The single owner of provisional-tab rollback. Every spawn-flow failure
//! that removes a tab it created MUST route through one of these helpers:
//! a concurrent or debounced save can persist the provisional tab before the
//! rollback runs, and an unpersisted removal would later read as an
//! unexplained reduction (refused save + resurrection on restart).

impl super::ServiceContext {
    /// Roll back a provisional spawn tab (no closed-history record) and
    /// persist the removal as an authorized reduction.
    pub(crate) fn rollback_provisional_tab(&self, ws_id: &str, tab_id: &str) {
        self.mux.write().rollback_spawn_tab(ws_id, tab_id);
        super::session_persist::persist_session_after_close(self);
    }

    /// Same guarantee for rollbacks that retire the provisional tab through
    /// the ordinary mux close (records closed history).
    pub(crate) fn rollback_provisional_tab_via_close(&self, tab_id: &str) {
        let _ = self.mux.write().close_tab(tab_id);
        super::session_persist::persist_session_after_close(self);
    }
}
