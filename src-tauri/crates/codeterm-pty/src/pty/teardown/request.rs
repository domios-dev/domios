use super::{ActivePtyReaper, PtySession};
use std::sync::Arc;
use std::time::Duration;

struct Request {
    pty_id: String,
    session: Arc<PtySession>,
    entire_tree: bool,
}

pub struct PtyReaper {
    request: Option<Request>,
}

impl PtyReaper {
    pub(crate) fn new(pty_id: String, session: Arc<PtySession>, entire_tree: bool) -> Self {
        Self {
            request: Some(Request {
                pty_id,
                session,
                entire_tree,
            }),
        }
    }

    pub fn pty_id(&self) -> &str {
        self.request
            .as_ref()
            .map_or("", |request| request.pty_id.as_str())
    }

    fn start(mut self) -> Result<ActivePtyReaper, String> {
        self.request
            .take()
            .ok_or_else(|| "PTY teardown already started".to_string())?
            .start()
    }

    pub fn wait_for_restart(self, grace: Duration) -> Result<(), String> {
        self.start()?.wait_for_restart(grace)
    }

    pub fn wait_bounded(self, timeout: Duration) -> Result<(), String> {
        self.start()?.wait_bounded(timeout)
    }

    pub fn wait_for_termination_bounded(self, timeout: Duration) -> Result<(), String> {
        self.start()?.wait_for_termination_bounded(timeout)
    }

    pub fn wait_for_termination(self) -> Result<(), String> {
        self.start()?.wait_for_termination()
    }

    pub fn reap_in_background(mut self) {
        if let Some(request) = self.request.take() {
            request.spawn();
        }
    }
}

impl Drop for PtyReaper {
    fn drop(&mut self) {
        if let Some(request) = self.request.take() {
            request.spawn();
        }
    }
}

impl Request {
    fn spawn(self) {
        codeterm_util::named_thread::spawn("pty-terminate", move || match self.start() {
            Ok(reaper) => reaper.reap_in_background(),
            Err(error) => log::error!("PTY teardown failed: {error}"),
        });
    }

    fn start(self) -> Result<ActivePtyReaper, String> {
        let Self {
            pty_id,
            session,
            entire_tree,
        } = self;
        session.write_queue.close();
        session.guarded_input.retire();
        let tree_killer = session
            .child
            .lock()
            .map_err(|error| format!("pty_tree_termination_failed: PTY {pty_id}: {error}"))?
            .clone_killer();
        let (kill_error, exit_pending_error) = match session.child.lock() {
            Ok(mut child) => {
                let leader_pid = child.process_id();
                let result = if entire_tree {
                    child.kill_tree()
                } else {
                    child.kill()
                };
                let group_errno = entire_tree.then(super::exited_group_errno).flatten();
                match super::classify_tree_kill(result, group_errno, || {
                    matches!(child.try_wait(), Ok(Some(_)))
                        || super::leader_exited_natively(leader_pid)
                }) {
                    Ok(()) => (None, None),
                    Err(error) if group_errno.is_some() && error.raw_os_error() == group_errno => {
                        (None, Some(error.to_string()))
                    }
                    Err(error) => (
                        Some(error)
                            .filter(super::super::manager::kill_failure_blocks_teardown)
                            .map(|error| error.to_string()),
                        None,
                    ),
                }
            }
            Err(error) => (Some(format!("child mutex poisoned: {error}")), None),
        };
        #[cfg(not(windows))]
        if let Ok(mut reader) = session.reader.lock() {
            let _ = reader.take();
        }
        if let Ok(mut tx) = session.backend_reader_tx.lock() {
            let _ = tx.take();
        }
        session.backend_output.close();
        // ConPTY handle is closed by the reaper's background thread, not here —
        // ClosePseudoConsole can block (see teardown.rs) and must not stall the caller.
        log::debug!(
            "Terminated PTY {} processes; console teardown queued",
            pty_id
        );
        #[cfg(target_os = "macos")]
        let child_pid = session
            .child
            .lock()
            .ok()
            .and_then(|child| child.process_id());
        Ok(ActivePtyReaper {
            pty_id,
            session,
            kill_error,
            exit_pending_error,
            tree_killer,
            #[cfg(target_os = "macos")]
            child_pid,
        })
    }
}
