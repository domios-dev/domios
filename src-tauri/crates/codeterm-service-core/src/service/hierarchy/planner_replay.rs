use parking_lot::Mutex;
use std::collections::HashMap;

/// One replay run in flight per tab; requests that arrive during a run collapse into one follow-up run.
#[derive(Default)]
pub(crate) struct PlannerReplayCoalescer {
    rerun_requested: Mutex<HashMap<String, bool>>,
}

impl PlannerReplayCoalescer {
    /// True when the caller owns a new run for `tab_id`.
    pub(crate) fn request(&self, tab_id: &str) -> bool {
        let mut tabs = self.rerun_requested.lock();
        match tabs.get_mut(tab_id) {
            Some(rerun) => {
                *rerun = true;
                false
            }
            None => {
                tabs.insert(tab_id.to_string(), false);
                true
            }
        }
    }

    /// True when a request arrived during the finished run, so the owner runs once more.
    pub(crate) fn finish(&self, tab_id: &str) -> bool {
        let mut tabs = self.rerun_requested.lock();
        match tabs.get_mut(tab_id) {
            Some(rerun) if *rerun => {
                *rerun = false;
                true
            }
            _ => {
                tabs.remove(tab_id);
                false
            }
        }
    }

    fn abandon(&self, tab_id: &str) {
        self.rerun_requested.lock().remove(tab_id);
    }

    /// Runs `replay` until no request is pending; a panicking run releases the tab for the next request.
    pub(crate) fn drain(&self, tab_id: &str, mut replay: impl FnMut()) {
        struct ReleaseOnUnwind<'a> {
            coalescer: &'a PlannerReplayCoalescer,
            tab_id: &'a str,
            armed: bool,
        }
        impl Drop for ReleaseOnUnwind<'_> {
            fn drop(&mut self) {
                if self.armed {
                    self.coalescer.abandon(self.tab_id);
                }
            }
        }
        let mut guard = ReleaseOnUnwind {
            coalescer: self,
            tab_id,
            armed: true,
        };
        loop {
            replay();
            if !self.finish(tab_id) {
                break;
            }
        }
        guard.armed = false;
    }
}

#[cfg(test)]
mod tests;
