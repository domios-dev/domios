//! Per-tab tailer handle registry. One tailer per tab, refcounted by
//! attached clients. A phone and a desktop can both enter chat mode without
//! one client's `stop` killing the other's stream.
//!
//! An entry is only reusable when it is BOTH alive (its polling thread is
//! still running) AND bound to the caller's current `session_id`. A tailer
//! that died on its own (session file vanished, provider path went None) or
//! that points at a session the tab has since rolled past is treated as
//! absent: it is shut down and the caller spawns a fresh one. Without this a
//! dead-but-registered corpse makes `acquire` return `true` forever, so the
//! live delta stream stays dead and reopening chat never recovers.

use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::tailer::TailerHandle;

struct Entry {
    handle: TailerHandle,
    subscribers: HashSet<String>,
    /// The agent session this tailer is bound to. A tab can roll to a new
    /// backing session (Codex resume mints a fresh rollout); the old tailer is
    /// then stale even though it is still alive.
    session_id: String,
}

impl Entry {
    /// Reusable iff the polling thread is alive and bound to `session_id`.
    fn usable(&self, session_id: &str) -> bool {
        self.session_id == session_id && self.handle.is_alive()
    }
}

#[derive(Default)]
pub struct ChatRegistry {
    inner: Mutex<HashMap<String, Entry>>,
}

impl ChatRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Attach `subscriber_id` to a live tailer bound to `session_id`.
    /// Returns `true` only when such a tailer exists.
    /// A dead or stale-session entry is dropped (and its thread stopped) and
    /// `false` is returned so the caller spawns a fresh tailer.
    pub fn acquire(&self, tab_id: &str, session_id: &str, subscriber_id: &str) -> bool {
        let mut g = self.inner.lock();
        match g.get(tab_id) {
            Some(entry) if entry.usable(session_id) => {
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
        subscriber_id: String,
        handle: TailerHandle,
    ) -> bool {
        let mut g = self.inner.lock();
        match g.get(&tab_id) {
            Some(entry) if entry.usable(&session_id) => {
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
    pub fn replace_if_current(&self, tab_id: &str, session_id: &str, handle: TailerHandle) -> bool {
        let old = {
            let mut g = self.inner.lock();
            let Some(entry) = g.get(tab_id) else {
                drop(g);
                handle.shutdown();
                return false;
            };
            if entry.session_id != session_id || !entry.handle.is_alive() {
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
            if !entry.subscribers.is_empty() {
                return;
            }
            g.remove(tab_id).map(|entry| entry.handle)
        };
        if let Some(handle) = h {
            handle.shutdown();
        }
    }

    pub fn shutdown_tab(&self, tab_id: &str) {
        let h = self.inner.lock().remove(tab_id).map(|entry| entry.handle);
        if let Some(handle) = h {
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
        assert!(registry.insert("tab".into(), SID.into(), "desktop".into(), handle_at(42)));
        assert!(registry.acquire("tab", SID, "web"));
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
        assert!(registry.insert("tab".into(), SID.into(), "desktop".into(), handle_at(10)));
        assert!(!registry.insert("tab".into(), SID.into(), "desktop".into(), handle_at(99)));
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
        assert!(registry.insert("tab".into(), SID.into(), "desktop".into(), dead_handle()));
        assert!(
            !registry.acquire("tab", SID, "desktop"),
            "a dead-but-registered tailer must not be reused",
        );
        assert!(
            !registry.is_running("tab"),
            "the corpse must be evicted so the caller spawns fresh",
        );
        // The fresh tailer registers cleanly.
        assert!(registry.insert("tab".into(), SID.into(), "desktop".into(), handle_at(7)));
        assert_eq!(registry.current_offset("tab"), Some(7));
    }

    #[test]
    fn acquire_drops_stale_session_tailer() {
        let registry = ChatRegistry::default();
        assert!(registry.insert(
            "tab".into(),
            "old-sid".into(),
            "desktop".into(),
            handle_at(5)
        ));
        assert!(
            !registry.acquire("tab", "new-sid", "desktop"),
            "a tailer bound to a rolled-past session is stale",
        );
        assert!(!registry.is_running("tab"));
        assert!(registry.insert(
            "tab".into(),
            "new-sid".into(),
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
        assert!(registry.insert("tab".into(), "old-sid".into(), "desktop".into(), watch));

        assert!(
            !registry.acquire("tab", "new-sid", "desktop"),
            "rotation must evict the stale watch rather than reuse it",
        );
        assert!(stopped.load(Ordering::Acquire));
        assert!(!registry.is_running("tab"));
    }

    #[test]
    fn shutdown_tab_stops_even_with_multiple_subscribers() {
        let registry = ChatRegistry::default();
        assert!(registry.insert("tab".into(), SID.into(), "desktop".into(), handle_at(42)));
        assert!(registry.acquire("tab", SID, "web"));
        registry.shutdown_tab("tab");
        assert!(!registry.is_running("tab"));
    }

    #[test]
    fn insert_replaces_dead_tailer() {
        let registry = ChatRegistry::default();
        assert!(registry.insert("tab".into(), SID.into(), "desktop".into(), dead_handle()));
        assert!(
            registry.insert("tab".into(), SID.into(), "desktop".into(), handle_at(33)),
            "a fresh tailer must replace a dead corpse rather than be discarded",
        );
        assert_eq!(registry.current_offset("tab"), Some(33));
    }

    #[test]
    fn rapid_start_stop_churn_keeps_one_live_tailer() {
        let registry = ChatRegistry::default();
        for _ in 0..50 {
            if !registry.acquire("tab", SID, "desktop") {
                registry.insert("tab".into(), SID.into(), "desktop".into(), handle_at(1));
            }
            registry.stop("tab", "desktop");
        }
        // Final mount with no matching stop leaves a single live tailer.
        if !registry.acquire("tab", SID, "desktop") {
            registry.insert("tab".into(), SID.into(), "desktop".into(), handle_at(1));
        }
        assert!(registry.is_running("tab"));
        assert!(
            registry.acquire("tab", SID, "web"),
            "the surviving tailer is alive and reusable",
        );
    }

    #[test]
    fn unknown_lease_stop_is_a_noop() {
        let registry = ChatRegistry::default();
        assert!(registry.insert("tab".into(), SID.into(), "desktop".into(), handle_at(42)));
        registry.stop("tab", "never-acquired");
        assert!(registry.is_running("tab"));
        registry.stop("tab", "desktop");
        assert!(!registry.is_running("tab"));
    }

    #[test]
    fn replacement_preserves_live_leases() {
        let registry = ChatRegistry::default();
        assert!(registry.insert("tab".into(), SID.into(), "desktop".into(), handle_at(1)));
        assert!(registry.acquire("tab", SID, "web"));
        assert!(registry.replace_if_current("tab", SID, handle_at(2)));
        registry.stop("tab", "desktop");
        assert!(registry.is_running("tab"));
        registry.stop("tab", "web");
        assert!(!registry.is_running("tab"));
    }
}
