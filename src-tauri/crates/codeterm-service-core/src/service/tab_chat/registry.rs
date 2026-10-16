use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use super::tailer::TailerHandle;

const SOURCE_IDENTITY_SAMPLE_BYTES: u64 = 64;

/// One materialization of a tab's JSONL transcript as observed by its reader.
/// `absence_witness` names the send capture that saw the exact path absent
/// before this file existed, so every row read from it postdates that capture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceIncarnation {
    pub id: Arc<str>,
    pub absence_witness: Option<Arc<str>>,
}

pub enum SourceCapture {
    Present { len: u64 },
    Absent { witness: Arc<str> },
    Unreadable,
}

pub enum SourceObservation {
    Absent,
    Present {
        incarnation: Option<SourceIncarnation>,
        restart: bool,
    },
}

#[derive(Default)]
struct SourceState {
    bound: Option<(String, PathBuf)>,
    current: Option<SourceIncarnation>,
    current_identity: Option<SourceFileIdentity>,
    pending_absence: Option<Arc<str>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceFileIdentity {
    created: Option<SystemTime>,
    held_offset: u64,
    prefix: Vec<u8>,
    held_window: Vec<u8>,
}

fn source_file_identity(
    path: &Path,
    len: u64,
    created: Option<SystemTime>,
    held_offset: u64,
) -> io::Result<SourceFileIdentity> {
    let mut file = File::open(path)?;
    let prefix = read_identity_window(&mut file, 0, len.min(SOURCE_IDENTITY_SAMPLE_BYTES))?;
    let held_end = held_offset.min(len);
    let held_start = held_end.saturating_sub(SOURCE_IDENTITY_SAMPLE_BYTES);
    let held_window = read_identity_window(&mut file, held_start, held_end - held_start)?;
    Ok(SourceFileIdentity {
        created,
        held_offset,
        prefix,
        held_window,
    })
}

fn read_identity_window(file: &mut File, start: u64, len: u64) -> io::Result<Vec<u8>> {
    let mut bytes = vec![0; len as usize];
    if len > 0 {
        file.seek(SeekFrom::Start(start))?;
        file.read_exact(&mut bytes)?;
    }
    Ok(bytes)
}

/// Send captures and transcript readers stat the exact path under one lock, so
/// an absence a capture saw always precedes the presence a reader later sees.
#[derive(Default)]
pub struct JsonlSource {
    state: Mutex<SourceState>,
}

fn opaque_id() -> Arc<str> {
    uuid::Uuid::new_v4().to_string().into()
}

impl SourceState {
    fn binds(&self, session_id: &str, path: &Path) -> Option<bool> {
        self.bound
            .as_ref()
            .map(|(session, bound)| session == session_id && bound == path)
    }
}

impl JsonlSource {
    pub fn capture(&self, session_id: &str, path: &Path) -> SourceCapture {
        let mut guard = self.state.lock();
        let state = &mut *guard;
        if state.binds(session_id, path) != Some(true) {
            *state = SourceState {
                bound: Some((session_id.to_string(), path.to_path_buf())),
                ..SourceState::default()
            };
        }
        match std::fs::metadata(path) {
            Ok(metadata) => SourceCapture::Present {
                len: metadata.len(),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => SourceCapture::Absent {
                witness: state.pending_absence.get_or_insert_with(opaque_id).clone(),
            },
            Err(_) => SourceCapture::Unreadable,
        }
    }

    /// A reader holding `held` at byte `offset` restarts at zero whenever the
    /// file it was reading is no longer the observed incarnation.
    pub fn observe(
        &self,
        session_id: &str,
        path: &Path,
        held: Option<&SourceIncarnation>,
        offset: u64,
    ) -> SourceObservation {
        let mut guard = self.state.lock();
        let state = &mut *guard;
        let binds = state.binds(session_id, path);
        let metadata = match std::fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(error) => {
                if error.kind() == std::io::ErrorKind::NotFound && binds == Some(true) {
                    state.current = None;
                    state.current_identity = None;
                }
                return SourceObservation::Absent;
            }
        };
        let len = metadata.len();
        let created = metadata.created().ok();
        match binds {
            Some(false) => {
                return SourceObservation::Present {
                    incarnation: None,
                    restart: false,
                }
            }
            Some(true) => {}
            None => {
                *state = SourceState {
                    bound: Some((session_id.to_string(), path.to_path_buf())),
                    ..SourceState::default()
                };
            }
        }
        let held_is_current = held.is_some() && held == state.current.as_ref();
        let truncated = held_is_current && offset > len;
        let observed_identity = source_file_identity(path, len, created.clone(), offset);
        let Some(observed_identity) = observed_identity.ok() else {
            state.current = None;
            state.current_identity = None;
            return SourceObservation::Present {
                incarnation: None,
                restart: true,
            };
        };
        let identity_changed = state.current.is_some()
            && state.current_identity.as_ref().is_some_and(|identity| {
                source_file_identity(path, len, created, identity.held_offset)
                    .map_or(true, |observed| observed != *identity)
            });
        if state.pending_absence.is_some()
            || state.current.is_none()
            || truncated
            || identity_changed
        {
            let absence_witness = state.pending_absence.take();
            state.current = Some(SourceIncarnation {
                id: opaque_id(),
                absence_witness,
            });
        }
        if state
            .current_identity
            .as_ref()
            .is_none_or(|identity| identity.held_offset <= observed_identity.held_offset)
        {
            state.current_identity = Some(observed_identity);
        }
        let current = state.current.clone();
        let restart = match held {
            Some(held) => Some(held) != current.as_ref(),
            None => offset > len,
        };
        SourceObservation::Present {
            incarnation: current,
            restart,
        }
    }
}

struct Entry {
    handle: TailerHandle,
    subscribers: HashSet<String>,
    session_id: String,
    session_binding_nonce: Option<String>,
}

impl Entry {
    fn usable(&self, session_id: &str, session_binding_nonce: Option<&str>) -> bool {
        self.session_id == session_id
            && self.session_binding_nonce.as_deref() == session_binding_nonce
            && self.handle.is_alive()
    }
}

#[derive(Default)]
pub struct ChatRegistry {
    inner: Mutex<HashMap<String, Entry>>,
    sources: Mutex<HashMap<String, Arc<JsonlSource>>>,
    pub snapshot_flights: super::snapshot_flight::SnapshotFlights,
}

impl ChatRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn jsonl_source(&self, tab_id: &str) -> Arc<JsonlSource> {
        self.sources
            .lock()
            .entry(tab_id.to_string())
            .or_default()
            .clone()
    }

    /// Attach `subscriber_id` to a live tailer bound to `session_id`.
    /// Returns `true` only when such a tailer exists.
    /// A dead or stale-session entry is dropped (and its thread stopped) and
    /// `false` is returned so the caller spawns a fresh tailer.
    pub fn acquire(
        &self,
        tab_id: &str,
        session_id: &str,
        session_binding_nonce: Option<&str>,
        subscriber_id: &str,
    ) -> bool {
        let mut g = self.inner.lock();
        match g.get(tab_id) {
            Some(entry) if entry.usable(session_id, session_binding_nonce) => {
                g.get_mut(tab_id)
                    .unwrap()
                    .subscribers
                    .insert(subscriber_id.to_string());
                true
            }
            Some(_) => {
                let stale = g.remove(tab_id);
                drop(g);
                if let Some(entry) = stale {
                    entry.handle.shutdown();
                }
                false
            }
            None => false,
        }
    }

    /// Register a freshly spawned tailer for `tab_id`/`session_id`.
    ///
    /// Returns `true` if the new tailer was registered. Returns `false` only
    /// when a still-usable tailer for the same session raced in ahead of this
    /// caller — then the provided duplicate handle is shut down and the
    /// existing entry adopts `subscriber_id`. A dead or
    /// stale-session entry is replaced by the new handle.
    pub fn insert(
        &self,
        tab_id: String,
        session_id: String,
        session_binding_nonce: Option<String>,
        subscriber_id: String,
        handle: TailerHandle,
    ) -> bool {
        let mut g = self.inner.lock();
        match g.get(&tab_id) {
            Some(entry) if entry.usable(&session_id, session_binding_nonce.as_deref()) => {
                g.get_mut(&tab_id)
                    .unwrap()
                    .subscribers
                    .insert(subscriber_id);
                drop(g);
                handle.shutdown();
                false
            }
            _ => {
                let old = g.insert(
                    tab_id,
                    Entry {
                        handle,
                        subscribers: HashSet::from([subscriber_id]),
                        session_id,
                        session_binding_nonce,
                    },
                );
                drop(g);
                if let Some(entry) = old {
                    entry.handle.shutdown();
                }
                true
            }
        }
    }

    /// Replace this tab/session's pending watcher with its materialized tailer.
    /// The watcher owns the call, so dropping its handle signals without joining it.
    pub fn replace_if_current(
        &self,
        tab_id: &str,
        session_id: &str,
        session_binding_nonce: Option<&str>,
        handle: TailerHandle,
    ) -> bool {
        let old = {
            let mut g = self.inner.lock();
            let Some(entry) = g.get(tab_id) else {
                drop(g);
                handle.shutdown();
                return false;
            };
            if !entry.usable(session_id, session_binding_nonce) {
                drop(g);
                handle.shutdown();
                return false;
            }
            let subscribers = entry.subscribers.clone();
            g.insert(
                tab_id.to_string(),
                Entry {
                    handle,
                    subscribers,
                    session_id: session_id.to_string(),
                    session_binding_nonce: session_binding_nonce.map(str::to_string),
                },
            )
        };
        drop(old);
        true
    }

    /// Release `subscriber_id`; an unknown subscription cannot stop a shared tailer.
    pub fn stop(&self, tab_id: &str, subscriber_id: &str) {
        let h = {
            let mut g = self.inner.lock();
            let Some(entry) = g.get_mut(tab_id) else {
                return;
            };
            if !entry.subscribers.remove(subscriber_id) {
                return;
            }
            if !entry.subscribers.is_empty() || entry.handle.session_owned() {
                return;
            }
            g.remove(tab_id).map(|entry| entry.handle)
        };
        if let Some(handle) = h {
            handle.shutdown();
        }
    }

    pub fn shutdown_tab(&self, tab_id: &str) {
        self.sources.lock().remove(tab_id);
        let h = self.inner.lock().remove(tab_id).map(|entry| entry.handle);
        if let Some(handle) = h {
            handle.shutdown();
        }
    }

    pub fn shutdown_session(&self, tab_id: &str, session_id: &str) {
        let handle = {
            let mut entries = self.inner.lock();
            if entries
                .get(tab_id)
                .is_none_or(|entry| entry.session_id != session_id)
            {
                return;
            }
            entries.remove(tab_id).map(|entry| entry.handle)
        };
        if let Some(handle) = handle {
            handle.shutdown();
        }
    }

    pub fn is_running(&self, tab_id: &str) -> bool {
        self.inner.lock().contains_key(tab_id)
    }

    pub fn current_offset(&self, tab_id: &str) -> Option<u64> {
        self.inner
            .lock()
            .get(tab_id)
            .map(|entry| entry.handle.current_offset())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::thread;
    use std::time::Duration;

    const SID: &str = "sid-A";

    #[test]
    fn truncate_and_regrow_past_held_offset_starts_a_new_incarnation() {
        use std::io::Write;

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("session.jsonl");
        let original = b"0123456789-original-turn\n";
        std::fs::write(&path, original).unwrap();
        let source = JsonlSource::default();
        let SourceObservation::Present {
            incarnation: Some(first),
            ..
        } = source.observe(SID, &path, None, 0)
        else {
            panic!("initial file observation must establish an incarnation");
        };
        let held_offset = original.len() as u64;
        assert!(matches!(
            source.observe(SID, &path, Some(&first), held_offset),
            SourceObservation::Present { restart: false, .. }
        ));

        let replacement = b"abcdefghij-replaced-turn\n";
        assert_eq!(replacement.len(), original.len());
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&path)
            .unwrap();
        file.write_all(replacement).unwrap();
        file.sync_all().unwrap();

        let SourceObservation::Present {
            incarnation: Some(second),
            restart,
        } = source.observe(SID, &path, Some(&first), held_offset)
        else {
            panic!("regrown file observation must carry a fresh incarnation");
        };
        assert!(
            restart,
            "changed sampled bytes invalidate the held position"
        );
        assert_ne!(first.id, second.id);
        assert_eq!(second.absence_witness, None);
    }

    fn handle_at(offset: u64) -> TailerHandle {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        TailerHandle::new(
            stop,
            Arc::new(AtomicU64::new(offset)),
            thread::spawn(move || {
                while !thread_stop.load(Ordering::Acquire) {
                    thread::sleep(Duration::from_millis(5));
                }
            }),
        )
    }

    /// A handle whose polling thread has already exited — models a tailer that
    /// gave up on its own (session file vanished) while still registered.
    fn dead_handle() -> TailerHandle {
        let stop = Arc::new(AtomicBool::new(true));
        let h = TailerHandle::new(stop, Arc::new(AtomicU64::new(0)), thread::spawn(|| {}));
        while h.is_alive() {
            thread::sleep(Duration::from_millis(2));
        }
        h
    }

    #[test]
    fn two_leases_release_one_subscriber_at_a_time() {
        let registry = ChatRegistry::default();
        assert!(registry.insert(
            "tab".into(),
            SID.into(),
            None,
            "desktop".into(),
            handle_at(42)
        ));
        assert!(registry.acquire("tab", SID, None, "web"));
        assert!(registry.is_running("tab"));

        registry.stop("tab", "desktop");
        assert!(
            registry.is_running("tab"),
            "first client leaving must not stop shared tailer",
        );
        assert_eq!(registry.current_offset("tab"), Some(42));

        registry.stop("tab", "web");
        assert!(
            !registry.is_running("tab"),
            "last client leaving stops shared tailer",
        );
    }

    #[test]
    fn duplicate_lease_start_is_idempotent() {
        let registry = ChatRegistry::default();
        assert!(registry.insert(
            "tab".into(),
            SID.into(),
            None,
            "desktop".into(),
            handle_at(10)
        ));
        assert!(!registry.insert(
            "tab".into(),
            SID.into(),
            None,
            "desktop".into(),
            handle_at(99)
        ));
        assert_eq!(
            registry.current_offset("tab"),
            Some(10),
            "duplicate insert must keep original tailer",
        );

        registry.stop("tab", "desktop");
        assert!(!registry.is_running("tab"));
    }

    #[test]
    fn acquire_drops_dead_tailer_and_signals_respawn() {
        let registry = ChatRegistry::default();
        assert!(registry.insert(
            "tab".into(),
            SID.into(),
            None,
            "desktop".into(),
            dead_handle()
        ));
        assert!(
            !registry.acquire("tab", SID, None, "desktop"),
            "a dead-but-registered tailer must not be reused",
        );
        assert!(
            !registry.is_running("tab"),
            "the corpse must be evicted so the caller spawns fresh",
        );
        // The fresh tailer registers cleanly.
        assert!(registry.insert(
            "tab".into(),
            SID.into(),
            None,
            "desktop".into(),
            handle_at(7)
        ));
        assert_eq!(registry.current_offset("tab"), Some(7));
    }

    #[test]
    fn acquire_drops_stale_session_tailer() {
        let registry = ChatRegistry::default();
        assert!(registry.insert(
            "tab".into(),
            "old-sid".into(),
            None,
            "desktop".into(),
            handle_at(5)
        ));
        assert!(
            !registry.acquire("tab", "new-sid", None, "desktop"),
            "a tailer bound to a rolled-past session is stale",
        );
        assert!(!registry.is_running("tab"));
        assert!(registry.insert(
            "tab".into(),
            "new-sid".into(),
            None,
            "desktop".into(),
            handle_at(9)
        ));
        assert_eq!(registry.current_offset("tab"), Some(9));
    }

    #[test]
    fn rotation_stops_stale_pending_watch_before_replacement() {
        let registry = ChatRegistry::default();
        let stopped = Arc::new(AtomicBool::new(false));
        let watch_stop = stopped.clone();
        let watch = TailerHandle::new(
            watch_stop.clone(),
            Arc::new(AtomicU64::new(0)),
            thread::spawn(move || {
                while !watch_stop.load(Ordering::Acquire) {
                    thread::sleep(Duration::from_millis(5));
                }
            }),
        );
        assert!(registry.insert(
            "tab".into(),
            "old-sid".into(),
            None,
            "desktop".into(),
            watch
        ));

        assert!(
            !registry.acquire("tab", "new-sid", None, "desktop"),
            "rotation must evict the stale watch rather than reuse it",
        );
        assert!(stopped.load(Ordering::Acquire));
        assert!(!registry.is_running("tab"));
    }

    #[test]
    fn shutdown_tab_stops_even_with_multiple_subscribers() {
        let registry = ChatRegistry::default();
        assert!(registry.insert(
            "tab".into(),
            SID.into(),
            None,
            "desktop".into(),
            handle_at(42)
        ));
        assert!(registry.acquire("tab", SID, None, "web"));
        registry.shutdown_tab("tab");
        assert!(!registry.is_running("tab"));
    }

    #[test]
    fn insert_replaces_dead_tailer() {
        let registry = ChatRegistry::default();
        assert!(registry.insert(
            "tab".into(),
            SID.into(),
            None,
            "desktop".into(),
            dead_handle()
        ));
        assert!(
            registry.insert(
                "tab".into(),
                SID.into(),
                None,
                "desktop".into(),
                handle_at(33)
            ),
            "a fresh tailer must replace a dead corpse rather than be discarded",
        );
        assert_eq!(registry.current_offset("tab"), Some(33));
    }

    #[test]
    fn rapid_start_stop_churn_keeps_one_live_tailer() {
        let registry = ChatRegistry::default();
        for _ in 0..50 {
            if !registry.acquire("tab", SID, None, "desktop") {
                registry.insert(
                    "tab".into(),
                    SID.into(),
                    None,
                    "desktop".into(),
                    handle_at(1),
                );
            }
            registry.stop("tab", "desktop");
        }
        // Final mount with no matching stop leaves a single live tailer.
        if !registry.acquire("tab", SID, None, "desktop") {
            registry.insert(
                "tab".into(),
                SID.into(),
                None,
                "desktop".into(),
                handle_at(1),
            );
        }
        assert!(registry.is_running("tab"));
        assert!(
            registry.acquire("tab", SID, None, "web"),
            "the surviving tailer is alive and reusable",
        );
    }

    #[test]
    fn unknown_lease_stop_is_a_noop() {
        let registry = ChatRegistry::default();
        assert!(registry.insert(
            "tab".into(),
            SID.into(),
            None,
            "desktop".into(),
            handle_at(42)
        ));
        registry.stop("tab", "never-acquired");
        assert!(registry.is_running("tab"));
        registry.stop("tab", "desktop");
        assert!(!registry.is_running("tab"));
    }

    #[test]
    fn event_tailer_survives_subscriber_churn_until_session_rotation() {
        let registry = ChatRegistry::default();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let (wake, events) = std::sync::mpsc::sync_channel(1);
        let handle = TailerHandle::event_driven(
            stop.clone(),
            Arc::new(AtomicU64::new(42)),
            thread::spawn(move || {
                while events.recv().is_ok() {
                    if thread_stop.load(Ordering::Acquire) {
                        return;
                    }
                }
            }),
            wake,
        );
        assert!(registry.insert("tab".into(), SID.into(), None, "desktop".into(), handle));
        for _ in 0..50 {
            registry.stop("tab", "desktop");
            assert!(registry.acquire("tab", SID, None, "desktop"));
            assert_eq!(registry.current_offset("tab"), Some(42));
            assert!(!stop.load(Ordering::Acquire));
        }
        assert!(!registry.acquire("tab", "new-session", None, "desktop"));
        assert!(stop.load(Ordering::Acquire));
        assert!(!registry.is_running("tab"));
    }

    #[test]
    fn replacement_preserves_live_leases() {
        let registry = ChatRegistry::default();
        assert!(registry.insert(
            "tab".into(),
            SID.into(),
            None,
            "desktop".into(),
            handle_at(1)
        ));
        assert!(registry.acquire("tab", SID, None, "web"));
        assert!(registry.replace_if_current("tab", SID, None, handle_at(2)));
        registry.stop("tab", "desktop");
        assert!(registry.is_running("tab"));
        registry.stop("tab", "web");
        assert!(!registry.is_running("tab"));
    }
}

#[cfg(test)]
#[path = "registry_binding_tests.rs"]
mod binding_tests;
