use tauri::Manager;

use crate::multiplexer::state::SnapshotMergeReport;
use crate::session::store::SessionBackupInfo;
use crate::session::SessionState;

impl super::ServiceContext {
    fn session_store(&self) -> Result<SessionState, String> {
        Ok(self
            .app_handle
            .try_state::<SessionState>()
            .ok_or_else(|| "SessionState not registered".to_string())?
            .inner()
            .clone())
    }

    pub fn list_session_backups(&self) -> Result<Vec<SessionBackupInfo>, String> {
        let session = self.session_store()?;
        let store = session.lock();
        store.list_backups()
    }

    /// Additive desk recovery: merge a `backups/` snapshot into the live desk.
    /// Nothing live is replaced or closed; only missing workspaces/tabs return.
    pub fn restore_session_backup(&self, name: &str) -> Result<SnapshotMergeReport, String> {
        let snapshot = {
            let session = self.session_store()?;
            let store = session.lock();
            store.load_backup(name)?
        };
        let report = self.mux.write().merge_session_snapshot(&snapshot);
        log::info!(
            "[session-backup] restored '{}': +{} workspace(s), +{} tab(s), {} already present",
            name,
            report.workspaces_added,
            report.tabs_added,
            report.tabs_already_present
        );
        self.emit_state_changed();
        super::session_persist::persist_session_after_mutation(
            self,
            super::session_persist::SessionPersistUrgency::Prompt,
        )?;
        Ok(report)
    }
}
