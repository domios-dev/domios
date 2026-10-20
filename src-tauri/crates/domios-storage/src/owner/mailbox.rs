use super::lock;
use crate::error::StorageError;
use std::collections::VecDeque;
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::Instant;

pub(crate) struct Queued<J> {
    pub(crate) job: J,
    cost: usize,
    pub(super) enqueued: Instant,
}

/// One wake of an owner thread: already-queued jobs plus the coalesced maintenance signal.
pub(super) struct Popped<J> {
    pub(super) jobs: Vec<Queued<J>>,
    pub(super) closed: bool,
    pub(super) signaled: bool,
}

struct MailboxState<J> {
    queue: VecDeque<Queued<J>>,
    bytes: usize,
    open: bool,
    signaled: bool,
}

/// Bounded by command count and payload bytes; a full mailbox refuses, never discards.
pub(super) struct Mailbox<J> {
    state: Mutex<MailboxState<J>>,
    ready: Condvar,
    limit_commands: usize,
    limit_bytes: usize,
}

impl<J> Mailbox<J> {
    pub(super) fn new(limit_commands: usize, limit_bytes: usize) -> Self {
        Mailbox {
            state: Mutex::new(MailboxState {
                queue: VecDeque::new(),
                bytes: 0,
                open: true,
                signaled: false,
            }),
            ready: Condvar::new(),
            limit_commands,
            limit_bytes,
        }
    }

    pub(super) fn push(&self, job: J, cost: usize) -> Result<(), (J, StorageError)> {
        let mut state = lock(&self.state);
        if !state.open {
            return Err((job, StorageError::Closed));
        }
        if cost > self.limit_bytes {
            let limit = self.limit_bytes;
            return Err((job, StorageError::TooLarge { bytes: cost, limit }));
        }
        if state.queue.len() >= self.limit_commands || state.bytes + cost > self.limit_bytes {
            let error = StorageError::Backpressure {
                queued_commands: state.queue.len(),
                queued_bytes: state.bytes,
                limit_commands: self.limit_commands,
                limit_bytes: self.limit_bytes,
            };
            return Err((job, error));
        }
        state.bytes += cost;
        let enqueued = Instant::now();
        state.queue.push_back(Queued {
            job,
            cost,
            enqueued,
        });
        drop(state);
        self.ready.notify_one();
        Ok(())
    }

    /// Wakes the owner without a job: coalesced, allocation-free and outside admission bounds.
    pub(super) fn signal(&self) {
        lock(&self.state).signaled = true;
        self.ready.notify_all();
    }

    /// Blocks until work, a signal or close; groups only commands that are already queued.
    pub(super) fn pop(&self, max: usize, groupable: fn(&J) -> bool) -> Option<Popped<J>> {
        let mut state = lock(&self.state);
        while state.queue.is_empty() && state.open && !state.signaled {
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
        let signaled = std::mem::take(&mut state.signaled);
        let mut jobs = Vec::new();
        if let Some(first) = state.queue.pop_front() {
            let group = groupable(&first.job);
            jobs.push(first);
            while group
                && jobs.len() < max
                && state.queue.front().is_some_and(|next| groupable(&next.job))
            {
                if let Some(next) = state.queue.pop_front() {
                    jobs.push(next);
                }
            }
            state.bytes -= jobs.iter().map(|queued| queued.cost).sum::<usize>();
        } else if !signaled {
            return None;
        }
        Some(Popped {
            jobs,
            closed: !state.open,
            signaled,
        })
    }

    pub(super) fn close(&self) {
        lock(&self.state).open = false;
        self.ready.notify_all();
    }

    pub(super) fn close_and_take(&self) -> Vec<Queued<J>> {
        let taken: Vec<_> = {
            let mut state = lock(&self.state);
            state.open = false;
            state.bytes = 0;
            state.queue.drain(..).collect()
        };
        self.ready.notify_all();
        taken
    }
}
