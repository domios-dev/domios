//! Single owner of provisional-tab rollback: removal + authorized-reduction persist together.

use crate::multiplexer::types::TabCloseReason;

impl super::ServiceContext {
    /// Roll back a provisional spawn tab (no closed-history record) and persist:
    /// a concurrent/debounced save may already have captured it in last.json.
    pub(crate) fn rollback_provisional_tab(&self, ws_id: &str, tab_id: &str) {
        let provider_id = self.tab_launch_artifact_provider(tab_id);
        self.mux.write().rollback_spawn_tab(ws_id, tab_id);
        if self.mux.read().get_tab(tab_id).is_none() {
            if let Some(provider_id) = provider_id {
                self.release_tab_launch_artifacts(tab_id, &provider_id);
            }
        }
        super::session_persist::persist_session_after_close(self);
    }

    /// Same guarantee for rollbacks that retire the provisional tab through
    /// the ordinary mux close (records closed history).
    pub(crate) fn rollback_provisional_tab_via_close(&self, tab_id: &str) {
        let provider_id = self.tab_launch_artifact_provider(tab_id);
        let result = self
            .mux
            .write()
            .close_tab_with_reason(tab_id, TabCloseReason::SpawnRollback);
        if result.is_ok() {
            if let Some(provider_id) = provider_id {
                self.release_tab_launch_artifacts(tab_id, &provider_id);
            }
        }
        super::session_persist::persist_session_after_close(self);
    }

    pub(crate) fn rollback_failed_agent_spawn(&self, tab_id: &str, failure: &str) -> String {
        self.cancel_agent_startup_supervision(tab_id);
        super::launch_session_detection::clear_launch_marker(tab_id);
        let Err(close_error) = self.close_tab_with_reason(tab_id, TabCloseReason::SpawnRollback)
        else {
            return failure.to_string();
        };
        log::warn!(
            "[spawn] tab={} rollback close failed: {}",
            tab_id,
            close_error
        );

        let (tab_exists, workspace_id, membership_error) = {
            let mux = self.mux.read();
            let workspace_id = mux.find_workspace_for_tab(tab_id);
            let membership_error = workspace_id.as_deref().and_then(|workspace_id| {
                mux.ensure_workspace_membership_mutable(workspace_id).err()
            });
            (
                mux.get_tab(tab_id).is_some(),
                workspace_id,
                membership_error,
            )
        };
        if !tab_exists {
            super::session_persist::persist_session_after_close(self);
            self.emit_state_changed();
            return format!(
                "{failure}; close reported an error after removing Tab {tab_id}: {close_error}"
            );
        }
        if let Some(membership_error) = membership_error {
            return format!(
                "{failure}; cleanup is owned by the active workspace close for Tab {tab_id}: {membership_error}; close error: {close_error}"
            );
        }
        let Some(workspace_id) = workspace_id else {
            return format!(
                "{failure}; cleanup failed because Tab {tab_id} has no owning workspace; close error: {close_error}"
            );
        };

        let cleanup_error = self.finish_tab_close(tab_id).err();
        self.rollback_provisional_tab(&workspace_id, tab_id);
        self.emit_state_changed();
        if self.mux.read().get_tab(tab_id).is_some() {
            return format!(
                "{failure}; cleanup failed because Tab {tab_id} remains; close error: {close_error}"
            );
        }
        if let Some(cleanup_error) = cleanup_error {
            return format!(
                "{failure}; Tab {tab_id} was removed but ancillary cleanup failed: {cleanup_error}; close error: {close_error}"
            );
        }
        format!("{failure}; fallback cleanup recovered from close error: {close_error}")
    }

    pub(crate) fn rollback_empty_agent_launch(&self, tab_id: &str, provider_id: &str) -> String {
        let failure = format!(
            "Agent provider '{}' did not resolve a launch command",
            provider_id
        );
        self.rollback_failed_agent_spawn(tab_id, &failure)
    }
}
