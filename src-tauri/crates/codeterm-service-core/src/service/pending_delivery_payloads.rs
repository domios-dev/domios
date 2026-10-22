use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::multiplexer::types::PendingDeliveryRecord;
use sha2::{Digest, Sha256};

pub(crate) mod agent_deliveries;
pub mod user_sends;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PayloadStoreError {
    InvalidDigest,
    DigestMismatch { digest: String },
    Missing { digest: String },
    Io { digest: String, message: String },
}

impl std::fmt::Display for PayloadStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDigest => f.write_str("invalid pending-delivery digest"),
            Self::DigestMismatch { digest } => {
                write!(f, "pending delivery payload digest mismatch: {digest}")
            }
            Self::Missing { digest } => write!(f, "RestorePayloadMissing: {digest}"),
            Self::Io { digest, message } => {
                write!(f, "pending delivery payload {digest}: {message}")
            }
        }
    }
}

impl std::error::Error for PayloadStoreError {}

#[derive(Clone)]
pub struct PendingDeliveryPayloadStore {
    dir: PathBuf,
    sink: Arc<dyn LogPayloadEventSink>,
    pub user_sends: Arc<user_sends::UserSendStore>,
    pub(crate) agent_deliveries: Arc<agent_deliveries::AgentDeliveryStore>,
    pub(crate) readiness_events:
        Arc<super::output_feed::delivery_readiness::DeliveryReadinessEvents>,
}

pub(crate) trait LogPayloadEventSink: Send + Sync {
    fn emit(&self, event: &str, digest: &str);
}

struct ProductPayloadLogSink;

impl LogPayloadEventSink for ProductPayloadLogSink {
    fn emit(&self, event: &str, digest: &str) {
        log::info!("[pending-delivery] event={} digest={}", event, digest);
    }
}

pub(crate) fn live_payload_keys(
    snapshot: &crate::multiplexer::state::MultiplexerSnapshot,
) -> HashSet<String> {
    snapshot
        .tabs
        .values()
        .filter_map(|tab| tab.pending_delivery.as_ref())
        .flat_map(|record| [payload_key(record), record.content_digest.clone()])
        .collect()
}

fn payload_key(record: &PendingDeliveryRecord) -> String {
    format!(
        "{:x}.{}",
        Sha256::digest(record.record_id().as_bytes()),
        record.content_digest
    )
}

impl PendingDeliveryPayloadStore {
    pub(crate) fn open(
        data_dir: &Path,
        live_keys: &HashSet<String>,
    ) -> Result<Self, PayloadStoreError> {
        Self::open_with_sink(data_dir, live_keys, Arc::new(ProductPayloadLogSink))
    }

    pub(crate) fn open_with_sink(
        data_dir: &Path,
        live_keys: &HashSet<String>,
        sink: Arc<dyn LogPayloadEventSink>,
    ) -> Result<Self, PayloadStoreError> {
        let store =
            Self {
                dir: data_dir.join("pending-delivery-payloads"),
                sink,
                user_sends: Arc::new(user_sends::UserSendStore::open(data_dir).map_err(
                    |message| PayloadStoreError::Io {
                        digest: "user-store".into(),
                        message,
                    },
                )?),
                agent_deliveries: Arc::new(agent_deliveries::AgentDeliveryStore::open(data_dir)),
                readiness_events: Arc::default(),
            };
        fs::create_dir_all(&store.dir).map_err(|error| store.io("store", error))?;
        store.restrict_dir_permissions()?;
        store.sweep_orphans(live_keys)?;
        Ok(store)
    }

    pub(crate) fn write(
        &self,
        record: &PendingDeliveryRecord,
        payload: &str,
    ) -> Result<(), PayloadStoreError> {
        let digest = &record.content_digest;
        self.verify_digest(digest, payload.as_bytes())?;
        let path = self.path(record)?;
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                self.restrict_file_permissions(&path)?;
                file.write_all(payload.as_bytes())
                    .map_err(|error| self.io(digest, error))?;
                file.sync_all().map_err(|error| self.io(digest, error))?;
                self.sink.emit("write", digest);
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                self.resolve(record).map(|existing| {
                    if existing != payload {
                        Err(PayloadStoreError::DigestMismatch {
                            digest: digest.to_string(),
                        })
                    } else {
                        Ok(())
                    }
                })?
            }
            Err(error) => Err(self.io(digest, error)),
        }
    }

    pub(crate) fn resolve(
        &self,
        record: &PendingDeliveryRecord,
    ) -> Result<String, PayloadStoreError> {
        let path = self.path(record)?;
        let digest = &record.content_digest;
        let bytes = fs::read(&path)
            .or_else(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    fs::read(self.dir.join(digest))
                } else {
                    Err(error)
                }
            })
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => PayloadStoreError::Missing {
                    digest: digest.to_string(),
                },
                _ => self.io(digest, error),
            })?;
        self.verify_digest(digest, &bytes)?;
        self.sink.emit("resolve", digest);
        String::from_utf8(bytes).map_err(|error| PayloadStoreError::Io {
            digest: digest.to_string(),
            message: error.to_string(),
        })
    }

    pub(crate) fn delete(
        &self,
        record: &PendingDeliveryRecord,
        live_keys: &HashSet<String>,
    ) -> Result<(), PayloadStoreError> {
        let path = self.path(record)?;
        if !live_keys.contains(&record.content_digest) {
            self.delete_path(
                &self.dir.join(&record.content_digest),
                &record.content_digest,
            )?;
        }
        self.delete_path(&path, &record.content_digest)
    }

    fn delete_path(&self, path: &Path, digest: &str) -> Result<(), PayloadStoreError> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(self.io(digest, error)),
        }
    }

    pub(crate) fn free_terminal_pair(
        &self,
        record: &mut Option<crate::multiplexer::types::PendingDeliveryRecord>,
        decision: &str,
        live_keys: &HashSet<String>,
    ) -> Result<(), PayloadStoreError> {
        let Some(pending) = record.as_ref() else {
            return Ok(());
        };
        let digest = pending.content_digest.clone();
        self.delete(pending, live_keys)?;
        self.sink.emit(decision, &digest);
        *record = None;
        Ok(())
    }

    pub(crate) fn retire_replaced(
        &self,
        previous: Option<&PendingDeliveryRecord>,
        record: &PendingDeliveryRecord,
        live_keys: &HashSet<String>,
    ) -> Result<(), PayloadStoreError> {
        if let Some(previous) =
            previous.filter(|previous| payload_key(previous) != payload_key(record))
        {
            self.delete(previous, live_keys)?;
        }
        Ok(())
    }

    pub(crate) fn sweep_orphans(
        &self,
        live_keys: &HashSet<String>,
    ) -> Result<(), PayloadStoreError> {
        for entry in fs::read_dir(&self.dir).map_err(|error| self.io("store", error))? {
            let entry = entry.map_err(|error| self.io("store", error))?;
            let Some(digest) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if !live_keys.contains(&digest) {
                self.delete_path(&entry.path(), &digest)?;
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn path_for_test(&self, record: &PendingDeliveryRecord) -> PathBuf {
        self.path(record).unwrap()
    }

    fn path(&self, record: &PendingDeliveryRecord) -> Result<PathBuf, PayloadStoreError> {
        let digest = &record.content_digest;
        if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(PayloadStoreError::InvalidDigest);
        }
        Ok(self.dir.join(payload_key(record)))
    }

    fn verify_digest(&self, digest: &str, bytes: &[u8]) -> Result<(), PayloadStoreError> {
        let actual = format!("{:x}", Sha256::digest(bytes));
        if actual == digest {
            Ok(())
        } else {
            Err(PayloadStoreError::DigestMismatch {
                digest: digest.to_string(),
            })
        }
    }

    fn io(&self, digest: &str, error: std::io::Error) -> PayloadStoreError {
        PayloadStoreError::Io {
            digest: digest.to_string(),
            message: error.to_string(),
        }
    }

    #[cfg(unix)]
    fn restrict_dir_permissions(&self) -> Result<(), PayloadStoreError> {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700))
            .map_err(|error| self.io("store", error))
    }

    #[cfg(windows)]
    fn restrict_dir_permissions(&self) -> Result<(), PayloadStoreError> {
        restrict_windows_acl(&self.dir).map_err(|error| self.io("store", error))
    }

    #[cfg(unix)]
    fn restrict_file_permissions(&self, path: &Path) -> Result<(), PayloadStoreError> {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|error| self.io("payload", error))
    }

    #[cfg(windows)]
    fn restrict_file_permissions(&self, path: &Path) -> Result<(), PayloadStoreError> {
        let _ = path;
        Ok(())
    }

    #[cfg(not(any(unix, windows)))]
    fn restrict_dir_permissions(&self) -> Result<(), PayloadStoreError> {
        Ok(())
    }

    #[cfg(not(any(unix, windows)))]
    fn restrict_file_permissions(&self, _: &Path) -> Result<(), PayloadStoreError> {
        Ok(())
    }
}

#[cfg(windows)]
fn restrict_windows_acl(path: &Path) -> std::io::Result<()> {
    #[cfg(test)]
    ACL_SETUP_CALLS.with(|calls| calls.set(calls.get() + 1));
    codeterm_mem::decision::token_store::restrict_directory_permissions(path)
}

#[cfg(all(windows, test))]
thread_local! {
    static ACL_SETUP_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(all(windows, test))]
fn acl_setup_calls() -> usize {
    ACL_SETUP_CALLS.with(std::cell::Cell::get)
}

#[cfg(test)]
mod tests;
