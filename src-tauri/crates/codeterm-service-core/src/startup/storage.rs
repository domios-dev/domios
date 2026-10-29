use std::path::Path;
use std::time::Duration;

use crate::storage::{Storage, StorageConfig};

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

    pub fn unavailable_reason(&self) -> Option<&str> {
        match self {
            StorageAuthority::Ready(_) => None,
            StorageAuthority::Unavailable { reason, .. } => Some(reason),
        }
    }

    pub fn mark_unavailable(&mut self, reason: String) {
        if let Some(storage) = self.storage().cloned() {
            *self = StorageAuthority::Unavailable {
                storage: Some(storage),
                reason,
            };
        }
    }
}

pub fn open_instance_storage() -> StorageAuthority {
    open_instance_storage_at(crate::paths::data_dir())
}

pub(crate) fn open_instance_storage_at(root: impl AsRef<Path>) -> StorageAuthority {
    let root = root.as_ref();
    let storage = match Storage::open(root, StorageConfig::default()) {
        Ok(storage) => storage,
        Err(error) => return unavailable(None, format!("storage open failed: {error}")),
    };
    if let Err(reason) = seed_empty_desk(&storage) {
        return unavailable(Some(storage), reason);
    }
    StorageAuthority::Ready(storage)
}

// A fresh database has no desk row until its first snapshot, and startup readers require one.
fn seed_empty_desk(storage: &Storage) -> Result<(), String> {
    match storage.desk().snapshot().wait(STARTUP_DEADLINE) {
        Ok(_) => Ok(()),
        Err(crate::storage::StorageError::NotFound { .. }) => storage
            .desk()
            .import(crate::storage::DeskSnapshot {
                version: "1".into(),
                timestamp: 0,
                active_workspace_id: None,
                workspaces: Vec::new(),
                teams: Vec::new(),
                agent_session_identities: Vec::new(),
            })
            .wait(STARTUP_DEADLINE)
            .map(|_| ())
            .map_err(|error| format!("empty desk seed failed: {error}")),
        Err(error) => Err(format!("desk read failed: {error}")),
    }
}

fn unavailable(storage: Option<Storage>, reason: String) -> StorageAuthority {
    log::error!("[storage] unavailable at startup: {reason}");
    StorageAuthority::Unavailable { storage, reason }
}

pub fn shutdown_bounded(authority: &StorageAuthority) -> Result<(), String> {
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
mod tests;
