use std::sync::Arc;
use std::time::Instant;

use parking_lot::{Condvar, Mutex};

use super::manager::PtySession;

#[derive(Default)]
struct PtyTeardownState {
    child_exited: bool,
    reader_finished: bool,
    console_closed: bool,
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
                console_closed: true,
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
        self.wait_for_boundary(Some(deadline), true)
    }

    fn wait_for_boundary(
        &self,
        deadline: Option<Instant>,
        require_cleanup: bool,
    ) -> Result<(), String> {
        let mut state = self.state.lock();
        loop {
            if let Some(error) = state.failure.as_ref() {
                return Err(error.clone());
            }
            if teardown_boundary_reached(
                state.child_exited,
                state.reader_finished && state.console_closed,
                require_cleanup,
            ) {
                return Ok(());
            }
            let Some(deadline) = deadline else {
                self.changed.wait(&mut state);
                continue;
            };
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return Err("process tree did not complete before deadline".to_string());
            };
            if self.changed.wait_for(&mut state, remaining).timed_out() {
                return Err("process tree did not complete before deadline".to_string());
            }
        }
    }
}

fn teardown_boundary_reached(
    child_exited: bool,
    reader_finished: bool,
    require_cleanup: bool,
) -> bool {
    child_exited && (!require_cleanup || reader_finished)
}

pub struct PtyReaper {
    pub(super) pty_id: String,
    pub(super) session: Arc<PtySession>,
    pub(super) kill_error: Option<String>,
}

impl PtyReaper {
    pub fn pty_id(&self) -> &str {
        &self.pty_id
    }

    pub fn wait_bounded(self, timeout: std::time::Duration) -> Result<(), String> {
        self.wait_at_boundary(Some(timeout), true)
    }

    pub fn wait_for_termination_bounded(self, timeout: std::time::Duration) -> Result<(), String> {
        self.wait_at_boundary(Some(timeout), false)
    }

    pub fn wait_for_termination(self) -> Result<(), String> {
        self.wait_at_boundary(None, false)
    }

    fn wait_at_boundary(
        self,
        timeout: Option<std::time::Duration>,
        require_cleanup: bool,
    ) -> Result<(), String> {
        self.observe_exit();
        if let Some(error) = self.kill_error.as_ref() {
            return Err(format!(
                "PTY {} process-tree termination failed: {error}",
                self.pty_id
            ));
        }
        self.session
            .teardown
            .wait_for_boundary(
                timeout.map(|timeout| Instant::now() + timeout),
                require_cleanup,
            )
            .map_err(|error| format!("PTY {} reap failed: {error}", self.pty_id))?;
        let state = self.session.teardown.state.lock();
        if !require_cleanup && !(state.reader_finished && state.console_closed) {
            log::warn!(
                "PTY {} process tree terminated; reader/console cleanup still pending in background",
                self.pty_id
            );
        }
        Ok(())
    }

    pub fn reap_in_background(self) {
        self.observe_exit();
    }

    fn observe_exit(&self) {
        self.session.teardown.state.lock().console_closed = false;
        // ClosePseudoConsole needs its output drained even when no backend claimed the reader.
        #[cfg(windows)]
        if let Some(mut reader) = self
            .session
            .reader
            .lock()
            .ok()
            .and_then(|mut reader| reader.take())
        {
            self.session.teardown.mark_reader_started();
            let session = self.session.clone();
            std::thread::spawn(move || {
                if let Err(error) = std::io::copy(&mut reader, &mut std::io::sink()) {
                    if error.kind() != std::io::ErrorKind::BrokenPipe {
                        session
                            .teardown
                            .mark_child_failed(format!("output drain failed: {error}"));
                    }
                }
                drop(reader);
                session.teardown.mark_reader_finished();
            });
        }
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
        let session = self.session.clone();
        std::thread::spawn(move || {
            // ConPTY close must not hide observed termination of the killed process tree.
            if let Ok(mut pair) = session.pair.lock() {
                let _ = pair.take();
            }
            session.teardown.state.lock().console_closed = true;
            session.teardown.changed.notify_all();
        });
    }
}

#[cfg(test)]
mod tests;
