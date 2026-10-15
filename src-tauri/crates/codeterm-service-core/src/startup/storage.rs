use std::path::Path;
use std::time::Duration;

use crate::storage::{ImportOutcome, Storage, StorageConfig};

const STARTUP_DEADLINE: Duration = Duration::from_secs(30);
pub const STORAGE_UNAVAILABLE_MESSAGE: &str =
    "Storage unavailable, your data is preserved, see the incident.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StorageUnavailableError;

impl std::fmt::Display for StorageUnavailableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(STORAGE_UNAVAILABLE_MESSAGE)
    }
}

impl std::error::Error for StorageUnavailableError {}

#[derive(Clone)]
pub enum StorageAuthority {
    Ready(Storage),
    Unavailable {
        storage: Option<Storage>,
        reason: String,
    },
}

impl StorageAuthority {
    pub(crate) fn require_ready(&self) -> Result<&Storage, StorageUnavailableError> {
        self.storage().ok_or(StorageUnavailableError)
    }

    pub fn storage(&self) -> Option<&Storage> {
        match self {
            StorageAuthority::Ready(storage) => Some(storage),
            StorageAuthority::Unavailable { .. } => None,
        }
    }

    pub(crate) fn shutdown_handle(&self) -> Option<Storage> {
        match self {
            StorageAuthority::Ready(storage) => Some(storage.clone()),
            StorageAuthority::Unavailable { storage, .. } => storage.clone(),
        }
    }

    pub(crate) fn unavailable_reason(&self) -> Option<&str> {
        match self {
            StorageAuthority::Ready(_) => None,
            StorageAuthority::Unavailable { reason, .. } => Some(reason),
        }
    }

    pub(crate) fn mark_unavailable(&mut self, reason: String) {
        if let Some(storage) = self.storage().cloned() {
            *self = StorageAuthority::Unavailable {
                storage: Some(storage),
                reason,
            };
        }
    }
}

pub(crate) fn open_instance_storage() -> StorageAuthority {
    open_instance_storage_at(crate::paths::data_dir())
}

pub(crate) fn open_instance_storage_at(root: impl AsRef<Path>) -> StorageAuthority {
    let root = root.as_ref();
    let storage = match Storage::open(root, StorageConfig::default()) {
        Ok(storage) => storage,
        Err(error) => return unavailable(None, format!("storage open failed: {error}")),
    };
    match storage.legacy_import_marker().wait(STARTUP_DEADLINE) {
        Ok(marker) if marker.value.is_some() => StorageAuthority::Ready(storage),
        Ok(_) => match crate::storage::cutover_import_live(&storage) {
            Ok(ImportOutcome::Committed(receipt)) => {
                if let Err(error) = receipt.archive {
                    unavailable(
                        Some(storage),
                        format!("storage import archive check failed: {error:?}"),
                    )
                } else if let Err(error) = receipt.comparison {
                    unavailable(
                        Some(storage),
                        format!("storage import comparison failed: {error:?}"),
                    )
                } else {
                    StorageAuthority::Ready(storage)
                }
            }
            Ok(ImportOutcome::AlreadyCommitted(_)) => StorageAuthority::Ready(storage),
            Err(error) => unavailable(Some(storage), format!("storage import failed: {error}")),
        },
        Err(error) => unavailable(
            Some(storage),
            format!("storage marker read failed: {error}"),
        ),
    }
}

fn unavailable(storage: Option<Storage>, reason: String) -> StorageAuthority {
    log::error!("[storage] unavailable at startup: {reason}");
    StorageAuthority::Unavailable { storage, reason }
}

pub(crate) fn shutdown_bounded(authority: &StorageAuthority) -> Result<(), String> {
    let Some(storage) = authority.shutdown_handle() else {
        return Ok(());
    };
    storage
        .shutdown()
        .wait(STARTUP_DEADLINE)
        .map(|_| ())
        .map_err(|error| format!("storage shutdown was not confirmed: {error}"))
}

#[cfg(test)]
fn source_bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).expect("source bytes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::store::StorageDeskAdapter;
    use std::fs;

    const EMPTY_DESK: &[u8] =
        br#"{"version":"1","timestamp":1,"workspaces":[],"active_workspace_id":null,"teams":[]}"#;
    const LATE_EDIT_DESK: &[u8] = br#"{"version":"1","timestamp":2,"workspaces":[{"id":"late-workspace","name":"Late workspace","tabs":[],"active_tab_id":null}],"active_workspace_id":"late-workspace","teams":[]}"#;

    fn close(authority: StorageAuthority) {
        if let Some(storage) = authority.shutdown_handle() {
            storage.shutdown().wait(STARTUP_DEADLINE).expect("shutdown");
        }
    }

    #[test]
    fn storage_cutover_startup_imports_from_a_sealed_copy_without_changing_originals() {
        let root = tempfile::tempdir().expect("instance root");
        let source = root.path().join("sessions/last.json");
        fs::create_dir_all(source.parent().expect("sessions directory")).expect("sessions");
        fs::write(&source, EMPTY_DESK).expect("legacy snapshot");

        let authority = open_instance_storage_at(root.path());
        assert!(authority.storage().is_some(), "authority is ready");
        assert_eq!(source_bytes(&source), EMPTY_DESK);
        let storage = authority.storage().expect("ready storage");
        let marker = storage
            .legacy_import_marker()
            .wait(STARTUP_DEADLINE)
            .expect("marker read");
        assert!(marker.value.is_some(), "import marker committed");
        assert!(StorageDeskAdapter::from_storage(storage.clone())
            .snapshot()
            .expect("desk snapshot")
            .workspaces
            .is_empty());
        close(authority);
    }

    #[test]
    fn storage_cutover_startup_marker_keeps_later_json_edits_out_of_the_desk() {
        let root = tempfile::tempdir().expect("instance root");
        let first = open_instance_storage_at(root.path());
        assert!(first.storage().is_some(), "first install is ready");
        close(first);

        let source = root.path().join("sessions/last.json");
        fs::create_dir_all(source.parent().expect("sessions directory")).expect("sessions");
        fs::write(&source, LATE_EDIT_DESK).expect("late legacy edit");
        let second = open_instance_storage_at(root.path());
        let storage = second.storage().expect("marker keeps database authority");
        assert!(StorageDeskAdapter::from_storage(storage.clone())
            .snapshot()
            .expect("desk snapshot")
            .workspaces
            .is_empty());
        close(second);
    }

    #[test]
    fn restored_workspace_cwd_comes_from_state_db_after_cutover() {
        const LEGACY_DESK: &[u8] = br#"{"version":"1","timestamp":1,"active_workspace_id":"ws-cwd","teams":[],"workspaces":[{"id":"ws-cwd","name":"Project","tabs":[],"active_tab_id":null,"cwd":"legacy-cwd","provisioned_cwd":null}]}"#;
        let root = tempfile::tempdir().expect("instance root");
        let legacy_path = root.path().join("sessions/last.json");
        fs::create_dir_all(legacy_path.parent().expect("legacy directory")).unwrap();
        fs::write(&legacy_path, LEGACY_DESK).expect("legacy snapshot");

        let first = open_instance_storage_at(root.path());
        let storage = first.storage().expect("ready storage");
        let adapter = StorageDeskAdapter::from_storage(storage.clone());
        let state = adapter.state().expect("database desk state");
        let mut record = state.snapshot.workspaces[0].record.clone();
        record.cwd = Some("database-cwd".into());
        adapter
            .update_workspace(record, state.workspace_revisions["ws-cwd"])
            .expect("update database workspace cwd");
        drop(state);
        drop(adapter);
        close(first);

        fs::write(
            &legacy_path,
            br#"{"version":"1","timestamp":2,"active_workspace_id":"ws-cwd","teams":[],"workspaces":[{"id":"ws-cwd","name":"Edited JSON","tabs":[],"active_tab_id":null,"cwd":"late-json-cwd","provisioned_cwd":null}]}"#,
        )
        .expect("conflicting post-cutover JSON edit");
        let second = open_instance_storage_at(root.path());
        let storage = second.storage().expect("database remains authoritative");
        let snapshot = StorageDeskAdapter::from_storage(storage.clone())
            .snapshot()
            .expect("restored database desk");
        let restored =
            crate::multiplexer::state::MultiplexerState::from_session_snapshot(&snapshot);

        assert_eq!(
            restored.list_workspaces()[0].cwd.as_deref(),
            Some("database-cwd")
        );
        assert_eq!(restored.list_workspaces()[0].name, "Project");
        close(second);
    }

    #[test]
    fn storage_cutover_startup_refuses_a_second_owner_for_the_same_root() {
        let root = tempfile::tempdir().expect("instance root");
        let first = open_instance_storage_at(root.path());
        assert!(first.storage().is_some(), "ready storage");
        assert!(Storage::open(root.path(), StorageConfig::default()).is_err());
        close(first);
    }

    #[test]
    fn storage_cutover_startup_failed_import_is_unavailable_and_preserves_originals() {
        let root = tempfile::tempdir().expect("instance root");
        let source = root.path().join("crons.json");
        let original = b"{";
        fs::write(&source, original).expect("invalid source");

        let authority = open_instance_storage_at(root.path());
        assert!(authority.storage().is_none());
        assert!(authority
            .unavailable_reason()
            .is_some_and(|reason| reason.contains("storage import failed")));
        assert_eq!(source_bytes(&source), original);
        close(authority);
    }

    #[test]
    fn storage_cutover_unavailable_authority_returns_a_typed_error() {
        let authority = StorageAuthority::Unavailable {
            storage: None,
            reason: "database failed".into(),
        };

        let error = match authority.require_ready() {
            Ok(_) => panic!("unavailable authority must not expose Storage"),
            Err(error) => error,
        };
        assert_eq!(error, StorageUnavailableError);
        assert_eq!(error.to_string(), STORAGE_UNAVAILABLE_MESSAGE);
    }
}
