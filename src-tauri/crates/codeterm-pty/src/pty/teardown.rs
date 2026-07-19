use std::sync::Arc;
use std::time::Instant;

use parking_lot::{Condvar, Mutex};

use super::manager::PtySession;

#[derive(Default)]
struct PtyTeardownState {
    child_exited: bool,
    reader_finished: bool,
    failure: Option<String>,
}

pub struct PtyTeardownSignal {
    state: Mutex<PtyTeardownState>,
    changed: Condvar,
}

impl Default for PtyTeardownSignal {
    fn default() -> Self {
        Self {
            state: Mutex::new(PtyTeardownState {
                child_exited: false,
                reader_finished: true,
                failure: None,
            }),
            changed: Condvar::new(),
        }
    }
}

impl PtyTeardownSignal {
    pub fn mark_reader_started(&self) {
        self.state.lock().reader_finished = false;
    }

    pub fn mark_reader_finished(&self) {
        let mut state = self.state.lock();
        state.reader_finished = true;
        self.changed.notify_all();
    }

    pub(crate) fn mark_child_exited(&self) {
        self.state.lock().child_exited = true;
        self.changed.notify_all();
    }

    pub(crate) fn mark_child_failed(&self, error: String) {
        self.state.lock().failure = Some(error);
        self.changed.notify_all();
    }

    pub fn wait_for_completion(&self, deadline: Instant) -> Result<(), String> {
        let mut state = self.state.lock();
        loop {
            if let Some(error) = state.failure.as_ref() {
                return Err(error.clone());
            }
            if state.child_exited && state.reader_finished {
                return Ok(());
            }
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return Err("process tree did not complete before deadline".to_string());
            };
            if self.changed.wait_for(&mut state, remaining).timed_out() {
                return Err("process tree did not complete before deadline".to_string());
            }
        }
    }
}

pub struct PtyReaper {
    pub(super) pty_id: String,
    pub(super) session: Arc<PtySession>,
    pub(super) kill_error: Option<String>,
}

impl PtyReaper {
    pub fn wait_bounded(self, timeout: std::time::Duration) -> Result<(), String> {
        self.observe_exit();
        if let Some(error) = self.kill_error.as_ref() {
            return Err(format!(
                "PTY {} process-tree termination failed: {error}",
                self.pty_id
            ));
        }
        self.session
            .teardown
            .wait_for_completion(Instant::now() + timeout)
            .map_err(|error| format!("PTY {} reap failed: {error}", self.pty_id))
    }

    pub(crate) fn reap_in_background(self) {
        self.observe_exit();
    }

    fn observe_exit(&self) {
        let session = self.session.clone();
        let pty_id = self.pty_id.clone();
        std::thread::spawn(move || {
            let result = match session.child.lock() {
                Ok(mut child) => child.wait().map_err(|error| error.to_string()),
                Err(error) => Err(format!("child mutex poisoned: {error}")),
            };
            match result {
                Ok(_) => {
                    session.teardown.mark_child_exited();
                    log::info!("PTY {} process tree reaped", pty_id);
                }
                Err(error) => session.teardown.mark_child_failed(error),
            }
        });
    }
}
