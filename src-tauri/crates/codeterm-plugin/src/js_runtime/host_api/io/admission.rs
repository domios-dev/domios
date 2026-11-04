use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};

use parking_lot::Mutex;

/// Native host I/O threads that may run at once, across all plugins and per plugin.
const GLOBAL_LIMIT: usize = 16;
const PLUGIN_LIMIT: usize = 4;
/// Waiting jobs per plugin. Revocation withdraws a queued job, so every entry belongs to
/// a live call, and a plugin VM admits at most this many live calls.
const QUEUE_LIMIT: usize = crate::js_runtime::vm::operation::MAX_OPERATIONS;

static SHARED: LazyLock<Arc<Admission>> =
    LazyLock::new(|| Arc::new(Admission::new(GLOBAL_LIMIT, PLUGIN_LIMIT, QUEUE_LIMIT)));
static NEXT_TICKET: AtomicU64 = AtomicU64::new(0);

pub(super) type Launch = Box<dyn FnOnce(Permit) + Send>;
pub(super) type Ticket = u64;

pub(super) fn ticket() -> Ticket {
    NEXT_TICKET.fetch_add(1, Ordering::Relaxed)
}

/// Admits a started host I/O job now when capacity allows, otherwise queues it behind
/// earlier jobs until a permit frees or `withdraw` drops it; only a full queue refuses.
pub(super) fn submit(plugin: &str, ticket: Ticket, launch: Launch) -> Result<(), String> {
    SHARED.submit(plugin, ticket, launch)
}

/// Drops a still-queued job and everything its launch captured; started jobs are unaffected.
pub(super) fn withdraw(ticket: Ticket) {
    SHARED.withdraw(ticket);
}

struct Queued {
    plugin: String,
    ticket: Ticket,
    launch: Launch,
}

#[derive(Default)]
struct State {
    running: usize,
    active: HashMap<String, usize>,
    queued: VecDeque<Queued>,
}

pub(super) struct Admission {
    global: usize,
    plugin: usize,
    queue: usize,
    state: Mutex<State>,
    #[cfg(test)]
    queued_changed: parking_lot::Condvar,
}

impl Admission {
    pub(super) fn new(global: usize, plugin: usize, queue: usize) -> Self {
        Self {
            global,
            plugin,
            queue,
            state: Mutex::default(),
            #[cfg(test)]
            queued_changed: parking_lot::Condvar::new(),
        }
    }

    pub(super) fn submit(
        self: &Arc<Self>,
        plugin: &str,
        ticket: Ticket,
        launch: Launch,
    ) -> Result<(), String> {
        let permit = {
            let mut state = self.state.lock();
            // Release drains every eligible queued job under this lock, so a job that is
            // admissible here cannot overtake an earlier queued job of its plugin.
            if self.admits(&state, plugin) {
                self.take(&mut state, plugin)
            } else {
                let waiting = state
                    .queued
                    .iter()
                    .filter(|entry| entry.plugin == plugin)
                    .count();
                if waiting >= self.queue {
                    drop(state);
                    drop(launch);
                    return Err("host I/O job queue is full".into());
                }
                state.queued.push_back(Queued {
                    plugin: plugin.into(),
                    ticket,
                    launch,
                });
                #[cfg(test)]
                self.queued_changed.notify_all();
                return Ok(());
            }
        };
        launch(permit);
        Ok(())
    }

    pub(super) fn withdraw(&self, ticket: Ticket) {
        let withdrawn = {
            let mut state = self.state.lock();
            let index = state.queued.iter().position(|entry| entry.ticket == ticket);
            let withdrawn = index.and_then(|index| state.queued.remove(index));
            #[cfg(test)]
            self.queued_changed.notify_all();
            withdrawn
        };
        drop(withdrawn);
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
                .position(|entry| self.admits(&state, &entry.plugin))
            {
                let Some(entry) = state.queued.remove(index) else {
                    break;
                };
                ready.push((self.take(&mut state, &entry.plugin), entry.launch));
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
    pub(super) fn queued(&self, plugin: &str) -> usize {
        self.state
            .lock()
            .queued
            .iter()
            .filter(|entry| entry.plugin == plugin)
            .count()
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
                .filter(|entry| entry.plugin == plugin)
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
pub(super) fn queued(plugin: &str) -> usize {
    SHARED.queued(plugin)
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
