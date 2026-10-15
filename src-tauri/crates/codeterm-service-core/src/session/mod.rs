pub mod lifecycle {
    pub use codeterm_pty::session::lifecycle::*;
}
pub mod scrollback {
    pub use codeterm_pty::session::scrollback::*;
}
pub(crate) mod scrollback_restore;
pub(crate) mod scrollback_sync;
pub mod store;

pub use store::{SessionSnapshot, SessionStore};

use std::sync::Arc;

use parking_lot::Mutex;

pub type SessionState = Arc<Mutex<SessionStore>>;

pub fn load_current_session_from_storage(
    authority: &crate::startup::StorageAuthority,
) -> Result<Option<SessionSnapshot>, String> {
    let storage = authority
        .require_ready()
        .map_err(|error| error.to_string())?;
    crate::session::store::StorageDeskAdapter::from_storage(storage.clone())
        .snapshot()
        .map(Some)
}

#[cfg(test)]
mod storage_cutover_tests {
    use super::load_current_session_from_storage;
    use crate::startup::StorageAuthority;
    use crate::storage::{Storage, StorageConfig};
    use std::fs;
    use std::time::Duration;

    #[test]
    fn storage_cutover_load_session_without_name_uses_storage_instead_of_legacy_json() {
        let temp = tempfile::tempdir().unwrap();
        let legacy_dir = temp.path().join("sessions");
        fs::create_dir_all(&legacy_dir).unwrap();
        fs::write(
            legacy_dir.join("last.json"),
            br#"{"version":"1","timestamp":1,"active_workspace_id":"legacy","teams":[],"workspaces":[{"id":"legacy","name":"Legacy desk","tabs":[],"active_tab_id":null}]}"#,
        )
        .unwrap();

        let storage = Storage::open(temp.path(), StorageConfig::default()).unwrap();
        let stored_snapshot = super::SessionSnapshot {
            version: "storage-authority".into(),
            timestamp: 42,
            workspaces: Vec::new(),
            active_workspace_id: None,
            teams: Vec::new(),
        };
        crate::session::store::StorageDeskAdapter::from_storage(storage.clone())
            .import_snapshot(&stored_snapshot, Vec::new())
            .expect("persist Storage desk snapshot");
        let authority = StorageAuthority::Ready(storage.clone());
        let loaded = load_current_session_from_storage(&authority)
            .unwrap()
            .expect("Storage desk snapshot");

        assert_eq!(loaded.version, "storage-authority");
        assert_eq!(loaded.timestamp, 42);
        assert!(loaded.workspaces.is_empty());
        storage
            .shutdown()
            .wait(Duration::from_secs(30))
            .expect("Storage shutdown");
    }

    #[test]
    fn storage_cutover_load_session_without_name_reports_unavailable_authority() {
        let authority = StorageAuthority::Unavailable {
            storage: None,
            reason: "database failed".into(),
        };
        let error = load_current_session_from_storage(&authority).unwrap_err();
        assert_eq!(error, crate::startup::STORAGE_UNAVAILABLE_MESSAGE,);
    }
}

pub use scrollback_restore::load_restored_scrollback_tails;
