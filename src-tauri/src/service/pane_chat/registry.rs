//! Per-pane tailer handle registry. One tailer per pane, refcounted by
//! attached clients. A phone and a desktop can both enter chat mode without
//! one client's `stop` killing the other's stream.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

use super::tailer::TailerHandle;

struct Entry {
    handle: TailerHandle,
    refs: usize,
}

#[derive(Default)]
pub struct ChatRegistry {
    inner: Mutex<HashMap<String, Entry>>,
}

impl ChatRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Increment the subscriber count for an already-running tailer.
    pub fn acquire(&self, pane_id: &str) -> bool {
        let mut g = self.inner.lock();
        if let Some(entry) = g.get_mut(pane_id) {
            entry.refs += 1;
            return true;
        }
        false
    }

    /// Returns `true` if a new tailer was registered. `false` means a
    /// tailer was already running for `pane_id`; the provided duplicate
    /// handle is shut down and the existing entry's subscriber count is
    /// incremented.
    pub fn insert(&self, pane_id: String, handle: TailerHandle) -> bool {
        let mut g = self.inner.lock();
        if let Some(entry) = g.get_mut(&pane_id) {
            entry.refs += 1;
            drop(g);
            handle.shutdown();
            return false;
        }
        g.insert(pane_id, Entry { handle, refs: 1 });
        true
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

    #[test]
    fn stop_releases_one_subscriber_at_a_time() {
        let registry = ChatRegistry::default();
        assert!(registry.insert("pane".into(), handle_at(42)));
        assert!(registry.acquire("pane"));
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
        assert!(registry.insert("pane".into(), handle_at(10)));
        assert!(!registry.insert("pane".into(), handle_at(99)));
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
}
