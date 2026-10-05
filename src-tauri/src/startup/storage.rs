use std::path::Path;
use std::time::Duration;

use crate::storage::{ImportOutcome, Storage, StorageConfig};

const STARTUP_DEADLINE: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub(crate) enum StorageAuthority {
    Ready(Storage),
    Unavailable {
        storage: Option<Storage>,
        reason: String,
    },
}

impl StorageAuthority {
    pub(crate) fn storage(&self) -> Option<&Storage> {
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

fn open_instance_storage_at(root: impl AsRef<Path>) -> StorageAuthority {
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
                    unavailable(Some(storage), format!("storage import archive check failed: {error:?}"))
                } else if let Err(error) = receipt.comparison {
                    unavailable(Some(storage), format!("storage import comparison failed: {error:?}"))
                } else {
                    StorageAuthority::Ready(storage)
                }
            }
            Ok(ImportOutcome::AlreadyCommitted(_)) => StorageAuthority::Ready(storage),
            Err(error) => unavailable(Some(storage), format!("storage import failed: {error}")),
        },
        Err(error) => unavailable(Some(storage), format!("storage marker read failed: {error}")),
    }
}

fn unavailable(storage: Option<Storage>, reason: String) -> StorageAuthority {
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

    const EMPTY_DESK: &[u8] = br#"{"version":"1","timestamp":1,"workspaces":[],"active_workspace_id":null,"teams":[]}"#;
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
}
