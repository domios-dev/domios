use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

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

#[derive(Debug, Clone)]
pub(crate) struct PendingDeliveryPayloadStore {
    dir: PathBuf,
}

impl PendingDeliveryPayloadStore {
    pub(crate) fn open(
        data_dir: &Path,
        live_digests: &HashSet<String>,
    ) -> Result<Self, PayloadStoreError> {
        let store = Self {
            dir: data_dir.join("pending-delivery-payloads"),
        };
        fs::create_dir_all(&store.dir).map_err(|error| store.io("store", error))?;
        store.restrict_dir_permissions()?;
        store.sweep_orphans(live_digests)?;
        Ok(store)
    }

    pub(crate) fn write(&self, digest: &str, payload: &str) -> Result<(), PayloadStoreError> {
        self.verify_digest(digest, payload.as_bytes())?;
        let path = self.path(digest)?;
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                self.restrict_file_permissions(&file)?;
                file.write_all(payload.as_bytes())
                    .map_err(|error| self.io(digest, error))?;
                file.sync_all().map_err(|error| self.io(digest, error))?;
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                self.resolve(digest).map(|existing| {
                    if existing != payload {
                        Err(PayloadStoreError::DigestMismatch {
                            digest: digest.into(),
                        })
                    } else {
                        Ok(())
                    }
                })?
            }
            Err(error) => Err(self.io(digest, error)),
        }
    }

    pub(crate) fn resolve(&self, digest: &str) -> Result<String, PayloadStoreError> {
        let path = self.path(digest)?;
        let bytes = fs::read(&path).map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => PayloadStoreError::Missing {
                digest: digest.into(),
            },
            _ => self.io(digest, error),
        })?;
        self.verify_digest(digest, &bytes)?;
        String::from_utf8(bytes).map_err(|error| PayloadStoreError::Io {
            digest: digest.into(),
            message: error.to_string(),
        })
    }

    pub(crate) fn delete(&self, digest: &str) -> Result<(), PayloadStoreError> {
        let path = self.path(digest)?;
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(self.io(digest, error)),
        }
    }

    pub(crate) fn sweep_orphans(
        &self,
        live_digests: &HashSet<String>,
    ) -> Result<(), PayloadStoreError> {
        for entry in fs::read_dir(&self.dir).map_err(|error| self.io("store", error))? {
            let entry = entry.map_err(|error| self.io("store", error))?;
            let Some(digest) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if !live_digests.contains(&digest) {
                self.delete(&digest)?;
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn path_for_test(&self, digest: &str) -> PathBuf {
        self.path(digest).unwrap()
    }

    fn path(&self, digest: &str) -> Result<PathBuf, PayloadStoreError> {
        if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(PayloadStoreError::InvalidDigest);
        }
        Ok(self.dir.join(digest))
    }

    fn verify_digest(&self, digest: &str, bytes: &[u8]) -> Result<(), PayloadStoreError> {
        let actual = format!("{:x}", Sha256::digest(bytes));
        if actual == digest {
            Ok(())
        } else {
            Err(PayloadStoreError::DigestMismatch {
                digest: digest.into(),
            })
        }
    }

    fn io(&self, digest: &str, error: std::io::Error) -> PayloadStoreError {
        PayloadStoreError::Io {
            digest: digest.into(),
            message: error.to_string(),
        }
    }

    #[cfg(unix)]
    fn restrict_dir_permissions(&self) -> Result<(), PayloadStoreError> {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700))
            .map_err(|error| self.io("store", error))
    }

    #[cfg(not(unix))]
    fn restrict_dir_permissions(&self) -> Result<(), PayloadStoreError> {
        Ok(())
    }

    #[cfg(unix)]
    fn restrict_file_permissions(&self, file: &std::fs::File) -> Result<(), PayloadStoreError> {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|error| self.io("payload", error))
    }

    #[cfg(not(unix))]
    fn restrict_file_permissions(&self, _: &std::fs::File) -> Result<(), PayloadStoreError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multiplexer::types::{
        PendingDeliveryChannel, PendingDeliveryRecord, PendingDeliveryState,
    };

    fn digest(payload: &str) -> String {
        format!("{:x}", Sha256::digest(payload.as_bytes()))
    }

    #[test]
    fn payload_store_round_trip_across_restart() {
        let temp = tempfile::tempdir().unwrap();
        let payload = "the exact fallback brief";
        let digest = digest(payload);
        let store = PendingDeliveryPayloadStore::open(temp.path(), &HashSet::new()).unwrap();
        store.write(&digest, payload).unwrap();
        let live = HashSet::from([digest.clone()]);
        let reopened = PendingDeliveryPayloadStore::open(temp.path(), &live).unwrap();
        assert_eq!(reopened.resolve(&digest).unwrap(), payload);
    }

    #[test]
    fn missing_and_tampered_payloads_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let payload = "private brief";
        let digest = digest(payload);
        let store = PendingDeliveryPayloadStore::open(temp.path(), &HashSet::new()).unwrap();
        assert_eq!(
            store.resolve(&digest),
            Err(PayloadStoreError::Missing {
                digest: digest.clone()
            })
        );
        store.write(&digest, payload).unwrap();
        fs::write(store.path_for_test(&digest), "tampered").unwrap();
        assert_eq!(
            store.resolve(&digest),
            Err(PayloadStoreError::DigestMismatch { digest })
        );
    }

    #[test]
    fn orphan_sweep_is_idempotent() {
        let temp = tempfile::tempdir().unwrap();
        let live_payload = "live";
        let orphan_payload = "orphan";
        let live = digest(live_payload);
        let orphan = digest(orphan_payload);
        let store = PendingDeliveryPayloadStore::open(temp.path(), &HashSet::new()).unwrap();
        store.write(&live, live_payload).unwrap();
        store.write(&orphan, orphan_payload).unwrap();
        let live_set = HashSet::from([live.clone()]);
        store.sweep_orphans(&live_set).unwrap();
        store.sweep_orphans(&live_set).unwrap();
        assert_eq!(store.resolve(&live).unwrap(), live_payload);
        assert_eq!(
            store.resolve(&orphan),
            Err(PayloadStoreError::Missing { digest: orphan })
        );
    }

    #[test]
    fn confirm_transition_deletes_payload() {
        let temp = tempfile::tempdir().unwrap();
        let payload = "terminal fallback brief";
        let digest = digest(payload);
        let store = PendingDeliveryPayloadStore::open(temp.path(), &HashSet::new()).unwrap();
        store.write(&digest, payload).unwrap();
        let mut record =
            PendingDeliveryRecord::new(digest.clone(), PendingDeliveryChannel::InteractiveInput);

        record.confirm().unwrap();
        store.delete(&digest).unwrap();
        store.delete(&digest).unwrap();

        assert_eq!(record.state(), PendingDeliveryState::Confirmed);
        assert_eq!(
            store.resolve(&digest),
            Err(PayloadStoreError::Missing { digest })
        );
    }
}
