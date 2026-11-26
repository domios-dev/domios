use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;

use super::model::{PersistedOutboxRecord, PersistedSettledReceipt, PersistedUnconfirmedReceipt};
use super::types::{OutboxError, OutboxId};

const ACTIVE_DIR: &str = "active";
const UNCONFIRMED_DIR: &str = "unconfirmed";
const SETTLED_DIR: &str = "settled";
const MAX_JSON_FILE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Default)]
pub struct LoadedOutboxFiles {
    pub active: Vec<PersistedOutboxRecord>,
    pub unconfirmed: Vec<PersistedUnconfirmedReceipt>,
    pub settled: Vec<PersistedSettledReceipt>,
}

#[derive(Debug, Clone)]
pub struct FileOutboxStore {
    root: PathBuf,
}

impl FileOutboxStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn active_dir(&self) -> PathBuf {
        self.root.join(ACTIVE_DIR)
    }

    pub fn unconfirmed_dir(&self) -> PathBuf {
        self.root.join(UNCONFIRMED_DIR)
    }

    pub fn settled_dir(&self) -> PathBuf {
        self.root.join(SETTLED_DIR)
    }

    pub fn active_path(&self, sequence: u64, id: &OutboxId) -> PathBuf {
        self.active_dir().join(record_file_name(sequence, id))
    }

    pub fn unconfirmed_path(&self, sequence: u64, id: &OutboxId) -> PathBuf {
        self.unconfirmed_dir().join(record_file_name(sequence, id))
    }

    pub fn settled_path(&self, sequence: u64, id: &OutboxId) -> PathBuf {
        self.settled_dir().join(record_file_name(sequence, id))
    }

    pub async fn load_all(&self) -> Result<LoadedOutboxFiles, OutboxError> {
        let store = self.clone();
        run_blocking(move || store.load_all_sync()).await
    }

    pub async fn write_active(&self, record: &PersistedOutboxRecord) -> Result<(), OutboxError> {
        let path = self.active_path(record.frame.sequence, &record.frame.id);
        let bytes = encode_json(record)?;
        run_blocking(move || write_private_file(&path, &bytes)).await
    }

    pub async fn write_active_batch(
        &self,
        records: &[PersistedOutboxRecord],
    ) -> Result<(), OutboxError> {
        let mut encoded = Vec::with_capacity(records.len());
        for record in records {
            let path = self.active_path(record.frame.sequence, &record.frame.id);
            encoded.push((path, encode_json(record)?));
        }
        run_blocking(move || {
            let mut written: Vec<PathBuf> = Vec::with_capacity(encoded.len());
            for (path, bytes) in encoded {
                if let Err(error) = write_private_file(&path, &bytes) {
                    for rollback in written {
                        let _ = fs::remove_file(rollback);
                    }
                    return Err(error);
                }
                written.push(path);
            }
            Ok(())
        })
        .await
    }

    pub async fn remove_active(&self, sequence: u64, id: &OutboxId) -> Result<(), OutboxError> {
        let path = self.active_path(sequence, id);
        run_blocking(move || remove_file_if_present(&path)).await
    }

    pub async fn persist_unconfirmed_and_remove_active(
        &self,
        receipt: &PersistedUnconfirmedReceipt,
        stale_active_sequence: u64,
    ) -> Result<(), OutboxError> {
        let unconfirmed_path = self.unconfirmed_path(receipt.sequence, &receipt.id);
        let active_path = self.active_path(stale_active_sequence, &receipt.id);
        let bytes = encode_json(receipt)?;
        run_blocking(move || {
            write_private_file(&unconfirmed_path, &bytes)?;
            remove_file_if_present(&active_path)
        })
        .await
    }

    pub async fn persist_settled_and_remove_active(
        &self,
        receipt: Option<&PersistedSettledReceipt>,
        active_sequence: u64,
        id: &OutboxId,
        evicted: &[PersistedSettledReceipt],
    ) -> Result<(), OutboxError> {
        let settled_write = match receipt {
            Some(entry) => Some((
                self.settled_path(entry.sequence, &entry.id),
                encode_json(entry)?,
            )),
            None => None,
        };
        let active_path = self.active_path(active_sequence, id);
        let evicted_paths: Vec<PathBuf> = evicted
            .iter()
            .map(|entry| self.settled_path(entry.sequence, &entry.id))
            .collect();
        run_blocking(move || {
            if let Some((path, bytes)) = settled_write {
                write_private_file(&path, &bytes)?;
            }
            remove_file_if_present(&active_path)?;
            for path in evicted_paths {
                let _ = remove_file_if_present(&path);
            }
            Ok(())
        })
        .await
    }

    pub async fn remove_unconfirmed(
        &self,
        sequence: u64,
        id: &OutboxId,
    ) -> Result<(), OutboxError> {
        let path = self.unconfirmed_path(sequence, id);
        run_blocking(move || remove_file_if_present(&path)).await
    }

    fn load_all_sync(&self) -> Result<LoadedOutboxFiles, OutboxError> {
        let active_dir = self.active_dir();
        let unconfirmed_dir = self.unconfirmed_dir();
        let settled_dir = self.settled_dir();
        codeterm_util::private_file::create_dir_all(&active_dir).map_err(io_to_storage)?;
        codeterm_util::private_file::create_dir_all(&unconfirmed_dir).map_err(io_to_storage)?;
        codeterm_util::private_file::create_dir_all(&settled_dir).map_err(io_to_storage)?;

        let unconfirmed: Vec<PersistedUnconfirmedReceipt> = load_json_dir(&unconfirmed_dir)?;
        let settled: Vec<PersistedSettledReceipt> = load_json_dir(&settled_dir)?;

        let terminal_ids: HashSet<OutboxId> = unconfirmed
            .iter()
            .map(|entry| entry.id.clone())
            .chain(settled.iter().map(|entry| entry.id.clone()))
            .collect();

        let raw_active: Vec<(PathBuf, PersistedOutboxRecord)> =
            load_json_dir_with_paths(&active_dir)?;
        let mut active = Vec::with_capacity(raw_active.len());
        for (path, record) in raw_active {
            if terminal_ids.contains(&record.frame.id) {
                let _ = remove_file_if_present(&path);
            } else {
                active.push(record);
            }
        }

        Ok(LoadedOutboxFiles {
            active,
            unconfirmed,
            settled,
        })
    }
}

pub(crate) fn record_file_name(sequence: u64, id: &OutboxId) -> String {
    format!("{sequence:020}-{}.json", id.as_str())
}

pub(crate) fn write_private_file(path: &Path, bytes: &[u8]) -> Result<(), OutboxError> {
    codeterm_util::private_file::write_atomic(path, bytes).map_err(io_to_storage)?;
    if let Some(parent) = path.parent() {
        sync_parent_dir(parent);
    }
    Ok(())
}

pub(crate) fn remove_file_if_present(path: &Path) -> Result<(), OutboxError> {
    match fs::remove_file(path) {
        Ok(()) => {
            if let Some(parent) = path.parent() {
                sync_parent_dir(parent);
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_to_storage(error)),
    }
}

pub(crate) fn load_json_dir<T: DeserializeOwned>(dir: &Path) -> Result<Vec<T>, OutboxError> {
    Ok(load_json_dir_with_paths(dir)?
        .into_iter()
        .map(|(_, value)| value)
        .collect())
}

pub(crate) fn load_json_dir_with_paths<T: DeserializeOwned>(
    dir: &Path,
) -> Result<Vec<(PathBuf, T)>, OutboxError> {
    let entries = fs::read_dir(dir).map_err(io_to_storage)?;
    let mut loaded = Vec::new();
    for entry in entries {
        let entry = entry.map_err(io_to_storage)?;
        let path = entry.path();
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if name.starts_with('.') && name.ends_with(".tmp") {
            let _ = fs::remove_file(&path);
            continue;
        }
        if !name.ends_with(".json") {
            continue;
        }
        let metadata = fs::symlink_metadata(&path).map_err(io_to_storage)?;
        if !metadata.is_file() {
            continue;
        }
        if metadata.len() > MAX_JSON_FILE_BYTES {
            return Err(OutboxError::Storage {
                detail: format!(
                    "persisted outbox file {} exceeds size limit",
                    path.display()
                ),
            });
        }
        let bytes = fs::read(&path).map_err(io_to_storage)?;
        let parsed: T = serde_json::from_slice(&bytes).map_err(|error| OutboxError::Storage {
            detail: format!("corrupt outbox file {}: {error}", path.display()),
        })?;
        loaded.push((path, parsed));
    }
    Ok(loaded)
}

#[cfg(unix)]
pub(crate) fn sync_parent_dir(dir: &Path) {
    if let Ok(file) = fs::File::open(dir) {
        let _ = file.sync_all();
    }
}

#[cfg(not(unix))]
pub(crate) fn sync_parent_dir(_dir: &Path) {}

fn encode_json<T: Serialize>(value: &T) -> Result<Vec<u8>, OutboxError> {
    serde_json::to_vec(value).map_err(|error| OutboxError::Storage {
        detail: format!("outbox serialization failed: {error}"),
    })
}

fn io_to_storage(error: io::Error) -> OutboxError {
    OutboxError::Storage {
        detail: error.to_string(),
    }
}

async fn run_blocking<F, T>(f: F) -> Result<T, OutboxError>
where
    F: FnOnce() -> Result<T, OutboxError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|error| OutboxError::Storage {
            detail: format!("outbox storage task failed: {error}"),
        })?
}
