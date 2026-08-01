use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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

#[derive(Clone)]
pub(crate) struct PendingDeliveryPayloadStore {
    dir: PathBuf,
    sink: Arc<dyn LogPayloadEventSink>,
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

pub(crate) fn live_payload_digests(
    snapshot: &crate::multiplexer::state::MultiplexerSnapshot,
) -> HashSet<String> {
    snapshot
        .tabs
        .values()
        .filter_map(|tab| tab.pending_delivery.as_ref())
        .map(|record| record.content_digest.clone())
        .collect()
}

impl PendingDeliveryPayloadStore {
    pub(crate) fn open(
        data_dir: &Path,
        live_digests: &HashSet<String>,
    ) -> Result<Self, PayloadStoreError> {
        Self::open_with_sink(data_dir, live_digests, Arc::new(ProductPayloadLogSink))
    }

    pub(crate) fn open_with_sink(
        data_dir: &Path,
        live_digests: &HashSet<String>,
        sink: Arc<dyn LogPayloadEventSink>,
    ) -> Result<Self, PayloadStoreError> {
        let store = Self {
            dir: data_dir.join("pending-delivery-payloads"),
            sink,
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
                self.restrict_file_permissions(&path)?;
                file.write_all(payload.as_bytes())
                    .map_err(|error| self.io(digest, error))?;
                file.sync_all().map_err(|error| self.io(digest, error))?;
                self.sink.emit("write", digest);
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
        self.sink.emit("resolve", digest);
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

    pub(crate) fn free_terminal_pair(
        &self,
        record: &mut Option<crate::multiplexer::types::PendingDeliveryRecord>,
        decision: &str,
    ) -> Result<(), PayloadStoreError> {
        let Some(pending) = record.as_ref() else {
            return Ok(());
        };
        let digest = pending.content_digest.clone();
        self.delete(&digest)?;
        self.sink.emit(decision, &digest);
        *record = None;
        Ok(())
    }

    pub(crate) fn replace(
        &self,
        previous_digest: Option<&str>,
        digest: &str,
        payload: &str,
    ) -> Result<(), PayloadStoreError> {
        self.write(digest, payload)?;
        if previous_digest.is_some_and(|previous| previous != digest) {
            self.delete(previous_digest.unwrap())?;
        }
        Ok(())
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
    let username = std::env::var("USERNAME")
        .ok()
        .filter(|username| !username.is_empty())
        .ok_or_else(|| std::io::Error::other("USERNAME is unavailable for payload ACL"))?;
    let mut command = std::process::Command::new("icacls");
    command.args([
        path.as_os_str(),
        std::ffi::OsStr::new("/inheritance:r"),
        std::ffi::OsStr::new("/grant:r"),
        std::ffi::OsStr::new(&format!("{username}:(OI)(CI)(F)")),
    ]);
    crate::process_util::no_window_std(&mut command);
    let output = command.output()?;
    if !output.status.success() {
        Err(std::io::Error::other("icacls rejected payload ACL"))
    } else {
        let mut verify = std::process::Command::new("icacls");
        verify.arg(path);
        crate::process_util::no_window_std(&mut verify);
        let output = verify.output()?;
        let acl = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
        let user = username.to_ascii_lowercase();
        if acl.contains(&user)
            && acl.contains("(oi)")
            && acl.contains("(ci)")
            && !acl.contains("builtin\\users")
            && !acl.contains("everyone:")
        {
            Ok(())
        } else {
            Err(std::io::Error::other(
                "payload directory ACL is not restrictive",
            ))
        }
    }
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
mod tests {
    use super::*;
    use crate::multiplexer::types::{
        PendingDeliveryChannel, PendingDeliveryRecord, PendingDeliveryState,
    };

    fn digest(payload: &str) -> String {
        format!("{:x}", Sha256::digest(payload.as_bytes()))
    }

    #[derive(Default)]
    struct CapturedPayloadEvents(parking_lot::Mutex<Vec<(String, String)>>);

    impl LogPayloadEventSink for CapturedPayloadEvents {
        fn emit(&self, event: &str, digest: &str) {
            self.0.lock().push((event.into(), digest.into()));
        }
    }

    #[test]
    fn payload_store_logs_digest_only_via_sink() {
        let temp = tempfile::tempdir().unwrap();
        let payload = "payload-must-not-be-logged";
        let digest = digest(payload);
        let events = Arc::new(CapturedPayloadEvents::default());
        let store = PendingDeliveryPayloadStore::open_with_sink(
            temp.path(),
            &HashSet::new(),
            events.clone(),
        )
        .unwrap();

        store.write(&digest, payload).unwrap();
        assert_eq!(store.resolve(&digest).unwrap(), payload);

        let events = events.0.lock().clone();
        assert_eq!(
            events,
            vec![("write".into(), digest.clone()), ("resolve".into(), digest)]
        );
        assert!(events
            .iter()
            .all(|(event, captured_digest)| !event.contains(payload)
                && !captured_digest.contains(payload)));
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
    fn confirmed_terminal_frees_record_and_payload_together() {
        let temp = tempfile::tempdir().unwrap();
        let payload = "terminal fallback brief";
        let digest = digest(payload);
        let events = Arc::new(CapturedPayloadEvents::default());
        let store = PendingDeliveryPayloadStore::open_with_sink(
            temp.path(),
            &HashSet::new(),
            events.clone(),
        )
        .unwrap();
        store.write(&digest, payload).unwrap();
        let mut record = Some(PendingDeliveryRecord::new(
            digest.clone(),
            PendingDeliveryChannel::InteractiveInput,
        ));

        record.as_mut().unwrap().confirm().unwrap();
        store.free_terminal_pair(&mut record, "confirmed").unwrap();

        assert!(record.is_none());
        assert_eq!(
            store.resolve(&digest),
            Err(PayloadStoreError::Missing {
                digest: digest.clone()
            })
        );
        assert!(events
            .0
            .lock()
            .contains(&("confirmed".into(), digest)));
    }

    #[test]
    fn explicit_discard_frees_record_and_payload_together() {
        let temp = tempfile::tempdir().unwrap();
        let payload = "discarded fallback brief";
        let digest = digest(payload);
        let events = Arc::new(CapturedPayloadEvents::default());
        let store = PendingDeliveryPayloadStore::open_with_sink(
            temp.path(),
            &HashSet::new(),
            events.clone(),
        )
        .unwrap();
        store.write(&digest, payload).unwrap();
        let mut record = Some(PendingDeliveryRecord::new(
            digest.clone(),
            PendingDeliveryChannel::InteractiveInput,
        ));

        store
            .free_terminal_pair(&mut record, "explicit_discard")
            .unwrap();

        assert!(record.is_none());
        assert_eq!(
            store.resolve(&digest),
            Err(PayloadStoreError::Missing {
                digest: digest.clone()
            })
        );
        assert!(events
            .0
            .lock()
            .contains(&("explicit_discard".into(), digest)));
    }

    #[test]
    fn retry_exhaustion_frees_record_and_payload_together() {
        let temp = tempfile::tempdir().unwrap();
        let payload = "retry-exhausted fallback brief";
        let digest = digest(payload);
        let events = Arc::new(CapturedPayloadEvents::default());
        let store = PendingDeliveryPayloadStore::open_with_sink(
            temp.path(),
            &HashSet::new(),
            events.clone(),
        )
        .unwrap();
        store.write(&digest, payload).unwrap();
        let mut record = Some(PendingDeliveryRecord::new(
            digest.clone(),
            PendingDeliveryChannel::InteractiveInput,
        ));
        record.as_mut().unwrap().mark_unconfirmed();

        store
            .free_terminal_pair(&mut record, "retry_exhausted")
            .unwrap();

        assert!(record.is_none());
        assert_eq!(
            store.resolve(&digest),
            Err(PayloadStoreError::Missing {
                digest: digest.clone()
            })
        );
        assert!(events
            .0
            .lock()
            .contains(&("retry_exhausted".into(), digest)));
    }

    #[test]
    fn terminal_cleanup_failure_keeps_record_addressable() {
        let temp = tempfile::tempdir().unwrap();
        let store = PendingDeliveryPayloadStore::open(temp.path(), &HashSet::new()).unwrap();
        let mut record = Some(PendingDeliveryRecord::new(
            "invalid".into(),
            PendingDeliveryChannel::InteractiveInput,
        ));

        assert_eq!(
            store.free_terminal_pair(&mut record, "confirmed"),
            Err(PayloadStoreError::InvalidDigest)
        );
        assert!(record.is_some());
    }

    #[test]
    fn superseding_payload_removes_prior_digest() {
        let temp = tempfile::tempdir().unwrap();
        let first = "first brief";
        let replacement = "replacement brief";
        let first_digest = digest(first);
        let replacement_digest = digest(replacement);
        let store = PendingDeliveryPayloadStore::open(temp.path(), &HashSet::new()).unwrap();
        store.write(&first_digest, first).unwrap();
        store
            .replace(Some(&first_digest), &replacement_digest, replacement)
            .unwrap();

        assert_eq!(store.resolve(&replacement_digest).unwrap(), replacement);
        assert_eq!(
            store.resolve(&first_digest),
            Err(PayloadStoreError::Missing {
                digest: first_digest
            })
        );
    }

    #[test]
    fn payload_permissions_are_restricted() {
        let temp = tempfile::tempdir().unwrap();
        let payload = "permission fixture";
        let digest = digest(payload);
        let store = PendingDeliveryPayloadStore::open(temp.path(), &HashSet::new()).unwrap();
        store.write(&digest, payload).unwrap();
        let path = store.path_for_test(&digest);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        #[cfg(windows)]
        {
            let mut command = std::process::Command::new("icacls");
            crate::process_util::no_window_std(&mut command);
            let output = command.arg(path).output().unwrap();
            let acl = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
            assert!(acl.contains(&std::env::var("USERNAME").unwrap().to_ascii_lowercase()));
            assert!(!acl.contains("builtin\\users"));
            assert!(!acl.contains("everyone:"));
        }
    }

    #[test]
    fn write_and_replace_do_not_run_acl_setup_after_open() {
        let temp = tempfile::tempdir().unwrap();
        let first = "first payload";
        let second = "second payload";
        let first_digest = digest(first);
        let second_digest = digest(second);
        let store = PendingDeliveryPayloadStore::open(temp.path(), &HashSet::new()).unwrap();
        #[cfg(windows)]
        let calls_after_open = acl_setup_calls();

        store.write(&first_digest, first).unwrap();
        store
            .replace(Some(&first_digest), &second_digest, second)
            .unwrap();

        #[cfg(windows)]
        assert_eq!(acl_setup_calls(), calls_after_open);
    }

    #[test]
    fn restore_resolves_payload_into_launch_option() {
        let temp = tempfile::tempdir().unwrap();
        let payload = "restored exact fallback brief";
        let digest = digest(payload);
        let store = PendingDeliveryPayloadStore::open(temp.path(), &HashSet::new()).unwrap();
        store.write(&digest, payload).unwrap();
        let record =
            PendingDeliveryRecord::new(digest.clone(), PendingDeliveryChannel::InteractiveInput);

        let launch_option = record
            .state()
            .needs_delivery()
            .then(|| store.resolve(&record.content_digest))
            .transpose()
            .unwrap();

        assert_eq!(launch_option.as_deref(), Some(payload));
    }

    #[test]
    fn restore_missing_payload_structured_error() {
        let temp = tempfile::tempdir().unwrap();
        let payload = "unavailable fallback brief";
        let digest = digest(payload);
        let store = PendingDeliveryPayloadStore::open(temp.path(), &HashSet::new()).unwrap();
        let record =
            PendingDeliveryRecord::new(digest.clone(), PendingDeliveryChannel::InteractiveInput);
        let delivery_writes = std::sync::atomic::AtomicUsize::new(0);

        let error = store.resolve(&record.content_digest).unwrap_err();

        assert_eq!(error, PayloadStoreError::Missing { digest });
        assert_eq!(record.state(), PendingDeliveryState::Pending);
        assert_eq!(
            delivery_writes.load(std::sync::atomic::Ordering::Relaxed),
            0
        );
    }

    #[test]
    fn leakage_serialization_keeps_payload_out_of_snapshot() {
        let payload = "payload-must-never-enter-snapshots-or-logs";
        let digest = digest(payload);
        let (mut state, tab_id) = crate::multiplexer::state::MultiplexerState::new();
        state.get_tab_mut(&tab_id).unwrap().pending_delivery = Some(PendingDeliveryRecord::new(
            digest.clone(),
            PendingDeliveryChannel::InteractiveInput,
        ));
        let snapshot = state.to_session_snapshot();
        let serialized = serde_json::to_string(&snapshot).unwrap();

        assert!(serialized.contains(&digest));
        assert!(!serialized.contains(payload));
    }

    #[test]
    fn live_digest_projection_retains_every_addressable_state() {
        let (mut state, tab_id) = crate::multiplexer::state::MultiplexerState::new();
        let payload = "retryable launch assignment";
        let digest = digest(payload);
        let record =
            PendingDeliveryRecord::new(digest.clone(), PendingDeliveryChannel::InteractiveInput);
        state.get_tab_mut(&tab_id).unwrap().pending_delivery = Some(record);
        assert_eq!(
            live_payload_digests(&state.snapshot()),
            HashSet::from([digest.clone()])
        );

        state
            .get_tab_mut(&tab_id)
            .unwrap()
            .pending_delivery
            .as_mut()
            .unwrap()
            .mark_ingested();
        assert_eq!(
            live_payload_digests(&state.snapshot()),
            HashSet::from([digest.clone()])
        );

        state
            .get_tab_mut(&tab_id)
            .unwrap()
            .pending_delivery
            .as_mut()
            .unwrap()
            .mark_unconfirmed();
        assert_eq!(
            live_payload_digests(&state.snapshot()),
            HashSet::from([digest.clone()])
        );

        state
            .get_tab_mut(&tab_id)
            .unwrap()
            .pending_delivery
            .as_mut()
            .unwrap()
            .confirm()
            .unwrap();
        assert_eq!(
            live_payload_digests(&state.snapshot()),
            HashSet::from([digest])
        );
    }
}
