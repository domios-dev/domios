use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::{Condvar, Mutex};

use crate::pty::manager::PtySession;

const CAPACITY: usize = 64;
const WRITE_DEADLINE: Duration = Duration::from_secs(30);

#[derive(Default)]
struct QueueState {
    pending: VecDeque<&'static [u8]>,
    in_flight: bool,
    closed: bool,
    overflow: u64,
}

struct Shared {
    state: Mutex<QueueState>,
    changed: Condvar,
    pty_id: String,
    generation: u64,
}

pub(super) struct ReplyOwner {
    shared: Arc<Shared>,
}

impl ReplyOwner {
    pub(super) fn start(pty_id: &str, session: Arc<PtySession>) -> Self {
        Self::with_writer(pty_id, session.generation, move |reply| {
            session
                .write_queue
                .send_and_wait(reply.to_vec(), WRITE_DEADLINE)
        })
    }

    pub(super) fn with_writer(
        pty_id: &str,
        generation: u64,
        mut write: impl FnMut(&[u8]) -> Result<(), String> + Send + 'static,
    ) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(QueueState::default()),
            changed: Condvar::new(),
            pty_id: pty_id.to_string(),
            generation,
        });
        let worker = shared.clone();
        codeterm_util::named_thread::spawn(format!("pty-reply:{pty_id}:{generation}"), move || {
            loop {
                let reply = {
                    let mut state = worker.state.lock();
                    while state.pending.is_empty() && !state.closed {
                        worker.changed.wait(&mut state);
                    }
                    if state.closed {
                        return;
                    }
                    let Some(reply) = state.pending.pop_front() else {
                        continue;
                    };
                    state.in_flight = true;
                    reply
                };
                let result = write(reply);
                let mut state = worker.state.lock();
                state.in_flight = false;
                if let Err(error) = result {
                    state.closed = true;
                    state.pending.clear();
                    log::debug!(
                        "PTY reply writer closed: pty={} generation={} error={error}",
                        worker.pty_id,
                        worker.generation,
                    );
                }
                worker.changed.notify_all();
                if state.closed {
                    return;
                }
            }
        });
        Self { shared }
    }

    pub(super) fn enqueue(&self, reply: &'static [u8]) -> bool {
        let mut state = self.shared.state.lock();
        if state.closed {
            return false;
        }
        if state.pending.len() + usize::from(state.in_flight) == CAPACITY {
            if state.overflow == 0 {
                log::debug!(
                    "PTY reply queue overflow: pty={} generation={} capacity={CAPACITY}",
                    self.shared.pty_id,
                    self.shared.generation,
                );
            }
            state.overflow = state.overflow.saturating_add(1);
            return false;
        }
        state.pending.push_back(reply);
        self.shared.changed.notify_one();
        true
    }
}

impl Drop for ReplyOwner {
    fn drop(&mut self) {
        let mut state = self.shared.state.lock();
        state.closed = true;
        state.pending.clear();
        self.shared.changed.notify_all();
    }
}

#[cfg(test)]
mod tests;
