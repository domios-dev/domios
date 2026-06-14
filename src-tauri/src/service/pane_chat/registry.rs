//! Per-pane tailer handle registry. One tailer per pane, refcounted by
//! attached clients. A phone and a desktop can both enter chat mode without
//! one client's `stop` killing the other's stream.
//!
//! An entry is only reusable when it is BOTH alive (its polling thread is
//! still running) AND bound to the caller's current `session_id`. A tailer
//! that died on its own (session file vanished, provider path went None) or
//! that points at a session the pane has since rolled past is treated as
//! absent: it is shut down and the caller spawns a fresh one. Without this a
//! dead-but-registered corpse makes `acquire` return `true` forever, so the
//! live delta stream stays dead and reopening chat never recovers.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

use super::tailer::TailerHandle;

struct Entry {
    handle: TailerHandle,
    refs: usize,
    /// The agent session this tailer is bound to. A pane can roll to a new
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

    /// Attach one more subscriber to a live tailer bound to `session_id`.
    /// Returns `true` only when such a tailer exists (refcount incremented).
    /// A dead or stale-session entry is dropped (and its thread stopped) and
    /// `false` is returned so the caller spawns a fresh tailer.
    pub fn acquire(&self, pane_id: &str, session_id: &str) -> bool {
        let mut g = self.inner.lock();
        match g.get(pane_id) {
            Some(entry) if entry.usable(session_id) => {
                g.get_mut(pane_id).unwrap().refs += 1;
                true
            }
            Some(_) => {
                let stale = g.remove(pane_id);
                drop(g);
                if let Some(entry) = stale {
                    entry.handle.shutdown();
                }
                false
            }
            None => false,
        }
    }

    /// Register a freshly spawned tailer for `pane_id`/`session_id`.
    ///
    /// Returns `true` if the new tailer was registered. Returns `false` only
    /// when a still-usable tailer for the same session raced in ahead of this
    /// caller — then the provided duplicate handle is shut down and the
    /// existing entry's subscriber count is incremented. A dead or
    /// stale-session entry is replaced by the new handle.
    pub fn insert(&self, pane_id: String, session_id: String, handle: TailerHandle) -> bool {
        let mut g = self.inner.lock();
        match g.get(&pane_id) {
            Some(entry) if entry.usable(&session_id) => {
                g.get_mut(&pane_id).unwrap().refs += 1;
                drop(g);
                handle.shutdown();
                false
            }
            _ => {
                let old = g.insert(
                    pane_id,
                    Entry {
                        handle,
                        refs: 1,
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

    /// Release one subscriber for `pane_id`; the worker is stopped only when
    /// the last client leaves. Safe to call on a pane that never had a tailer.
    pub fn stop(&self, pane_id: &str) {
        let h = {
            let mut g = self.inner.lock();
            let Some(entry) = g.get_mut(pane_id) else {
                return;
            };
            if entry.refs > 1 {
                entry.refs -= 1;
                return;
            }
            g.remove(pane_id).map(|entry| entry.handle)
        };
        if let Some(handle) = h {
            handle.shutdown();
        }
    }

    pub fn shutdown_pane(&self, pane_id: &str) {
        let h = self.inner.lock().remove(pane_id).map(|entry| entry.handle);
        if let Some(handle) = h {
            handle.shutdown();
        }
    }

    pub fn is_running(&self, pane_id: &str) -> bool {
        self.inner.lock().contains_key(pane_id)
    }

    pub fn current_offset(&self, pane_id: &str) -> Option<u64> {
        self.inner
            .lock()
            .get(pane_id)
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
    fn stop_releases_one_subscriber_at_a_time() {
        let registry = ChatRegistry::default();
        assert!(registry.insert("pane".into(), SID.into(), handle_at(42)));
        assert!(registry.acquire("pane", SID));
        assert!(registry.is_running("pane"));

        registry.stop("pane");
        assert!(
            registry.is_running("pane"),
            "first client leaving must not stop shared tailer",
        );
        assert_eq!(registry.current_offset("pane"), Some(42));

        registry.stop("pane");
        assert!(
            !registry.is_running("pane"),
            "last client leaving stops shared tailer",
        );
    }

    #[test]
    fn duplicate_insert_counts_as_another_subscriber() {
        let registry = ChatRegistry::default();
        assert!(registry.insert("pane".into(), SID.into(), handle_at(10)));
        assert!(!registry.insert("pane".into(), SID.into(), handle_at(99)));
        assert_eq!(
            registry.current_offset("pane"),
            Some(10),
            "duplicate insert must keep original tailer",
        );

        registry.stop("pane");
        assert!(registry.is_running("pane"));
        registry.stop("pane");
        assert!(!registry.is_running("pane"));
    }

    #[test]
    fn acquire_drops_dead_tailer_and_signals_respawn() {
        let registry = ChatRegistry::default();
        assert!(registry.insert("pane".into(), SID.into(), dead_handle()));
        assert!(
            !registry.acquire("pane", SID),
            "a dead-but-registered tailer must not be reused",
        );
        assert!(
            !registry.is_running("pane"),
            "the corpse must be evicted so the caller spawns fresh",
        );
        // The fresh tailer registers cleanly.
        assert!(registry.insert("pane".into(), SID.into(), handle_at(7)));
        assert_eq!(registry.current_offset("pane"), Some(7));
    }

    #[test]
    fn acquire_drops_stale_session_tailer() {
        let registry = ChatRegistry::default();
        assert!(registry.insert("pane".into(), "old-sid".into(), handle_at(5)));
        assert!(
            !registry.acquire("pane", "new-sid"),
            "a tailer bound to a rolled-past session is stale",
        );
        assert!(!registry.is_running("pane"));
        assert!(registry.insert("pane".into(), "new-sid".into(), handle_at(9)));
        assert_eq!(registry.current_offset("pane"), Some(9));
    }

    #[test]
    fn shutdown_pane_stops_even_with_multiple_subscribers() {
        let registry = ChatRegistry::default();
        assert!(registry.insert("pane".into(), SID.into(), handle_at(42)));
        assert!(registry.acquire("pane", SID));
        registry.shutdown_pane("pane");
        assert!(!registry.is_running("pane"));
    }

    #[test]
    fn insert_replaces_dead_tailer() {
        let registry = ChatRegistry::default();
        assert!(registry.insert("pane".into(), SID.into(), dead_handle()));
        assert!(
            registry.insert("pane".into(), SID.into(), handle_at(33)),
            "a fresh tailer must replace a dead corpse rather than be discarded",
        );
        assert_eq!(registry.current_offset("pane"), Some(33));
    }

    #[test]
    fn rapid_start_stop_churn_keeps_one_live_tailer() {
        // Models StrictMode/HMR mount→unmount→mount churn: every start either
        // acquires the live tailer or spawns one; every stop releases. The
        // stream must survive — exactly one live tailer at the end.
        let registry = ChatRegistry::default();
        for _ in 0..50 {
            if !registry.acquire("pane", SID) {
                registry.insert("pane".into(), SID.into(), handle_at(1));
            }
            registry.stop("pane");
        }
        // Final mount with no matching stop leaves a single live tailer.
        if !registry.acquire("pane", SID) {
            registry.insert("pane".into(), SID.into(), handle_at(1));
        }
        assert!(registry.is_running("pane"));
        assert!(
            registry.acquire("pane", SID),
            "the surviving tailer is alive and reusable",
        );
    }
}
