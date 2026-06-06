//! Per-pane tailer handle registry. One tailer per pane in UI mode.
//! `insert` is idempotent — re-call with the same pane id returns
//! `false` and leaves the existing tailer alone. `stop` joins the
//! tailer thread before returning.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

use super::tailer::TailerHandle;

#[derive(Default)]
pub struct ChatRegistry {
    inner: Mutex<HashMap<String, TailerHandle>>,
}

impl ChatRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Returns `true` if a new tailer was registered. `false` means a
    /// tailer was already running for `pane_id` — caller may choose to
    /// no-op or send a fresh snapshot anyway.
    pub fn insert(&self, pane_id: String, handle: TailerHandle) -> bool {
        let mut g = self.inner.lock();
        if g.contains_key(&pane_id) {
            return false;
        }
        g.insert(pane_id, handle);
        true
    }

    /// Stop and join the tailer for `pane_id`, if any. Safe to call on
    /// a pane that never had a tailer (no-op).
    pub fn stop(&self, pane_id: &str) {
        let h = self.inner.lock().remove(pane_id);
        if let Some(h) = h {
            h.shutdown();
        }
    }

    pub fn is_running(&self, pane_id: &str) -> bool {
        self.inner.lock().contains_key(pane_id)
    }

    pub fn current_offset(&self, pane_id: &str) -> Option<u64> {
        self.inner.lock().get(pane_id).map(|h| h.current_offset())
    }
}
