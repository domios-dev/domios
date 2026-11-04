use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, LazyLock};

use parking_lot::Mutex;

/// Native host I/O threads that may run at once, across all plugins and per plugin.
const GLOBAL_LIMIT: usize = 16;
const PLUGIN_LIMIT: usize = 4;

static SHARED: LazyLock<Arc<Admission>> =
    LazyLock::new(|| Arc::new(Admission::new(GLOBAL_LIMIT, PLUGIN_LIMIT)));

pub(super) type Launch = Box<dyn FnOnce(Permit) + Send>;

/// Admits a started host I/O job now when capacity allows, otherwise queues it behind
/// earlier jobs; queued jobs never fail for capacity, their caller's deadline bounds them.
pub(super) fn submit(plugin: &str, launch: Launch) {
    SHARED.submit(plugin, launch);
}

#[derive(Default)]
struct State {
    running: usize,
    active: HashMap<String, usize>,
    queued: VecDeque<(String, Launch)>,
}

pub(super) struct Admission {
    global: usize,
    plugin: usize,
    state: Mutex<State>,
    #[cfg(test)]
    queued_changed: parking_lot::Condvar,
}

impl Admission {
    pub(super) fn new(global: usize, plugin: usize) -> Self {
        Self {
            global,
            plugin,
            state: Mutex::default(),
            #[cfg(test)]
            queued_changed: parking_lot::Condvar::new(),
        }
    }

    pub(super) fn submit(self: &Arc<Self>, plugin: &str, launch: Launch) {
        let permit = {
            let mut state = self.state.lock();
            // Release drains every eligible queued job under this lock, so a job that is
            // admissible here cannot overtake an earlier queued job of its plugin.
            if !self.admits(&state, plugin) {
                state.queued.push_back((plugin.into(), launch));
                #[cfg(test)]
                self.queued_changed.notify_all();
                return;
            }
            self.take(&mut state, plugin)
        };
        launch(permit);
    }

    fn admits(&self, state: &State, plugin: &str) -> bool {
        state.running < self.global && state.active.get(plugin).copied().unwrap_or(0) < self.plugin
    }

    fn take(self: &Arc<Self>, state: &mut State, plugin: &str) -> Permit {
        state.running += 1;
        *state.active.entry(plugin.into()).or_default() += 1;
        Permit {
            admission: self.clone(),
            plugin: plugin.into(),
        }
    }

    fn release(self: &Arc<Self>, plugin: &str) {
        let ready = {
            let mut state = self.state.lock();
            state.running -= 1;
            let remaining = state.active.get_mut(plugin).map(|count| {
                *count -= 1;
                *count
            });
            if remaining == Some(0) {
                state.active.remove(plugin);
            }
            let mut ready = Vec::new();
            while let Some(index) = state
                .queued
                .iter()
                .position(|(queued, _)| self.admits(&state, queued))
            {
                let Some((queued, launch)) = state.queued.remove(index) else {
                    break;
                };
                ready.push((self.take(&mut state, &queued), launch));
            }
            #[cfg(test)]
            self.queued_changed.notify_all();
            ready
        };
        for (permit, launch) in ready {
            launch(permit);
        }
    }

    #[cfg(test)]
    pub(super) fn wait_queued(
        &self,
        plugin: &str,
        count: usize,
        bound: std::time::Duration,
    ) -> bool {
        let deadline = std::time::Instant::now() + bound;
        let mut state = self.state.lock();
        loop {
            let queued = state
                .queued
                .iter()
                .filter(|(queued, _)| queued == plugin)
                .count();
            if queued >= count {
                return true;
            }
            if self
                .queued_changed
                .wait_until(&mut state, deadline)
                .timed_out()
            {
                return false;
            }
        }
    }
}

#[cfg(test)]
pub(super) fn wait_queued(plugin: &str, count: usize, bound: std::time::Duration) -> bool {
    SHARED.wait_queued(plugin, count, bound)
}

/// Capacity held by one running host I/O job; dropping it admits the oldest eligible queued job.
pub(super) struct Permit {
    admission: Arc<Admission>,
    plugin: String,
}

impl Drop for Permit {
    fn drop(&mut self) {
        self.admission.release(&self.plugin);
    }
}

#[cfg(test)]
mod tests;
