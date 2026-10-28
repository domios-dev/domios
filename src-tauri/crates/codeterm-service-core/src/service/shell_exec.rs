use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use tokio::sync::oneshot;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecCompletion {
    CommandFinished { exit_code: Option<i32> },
    ShellExited { exit_code: Option<i32> },
}

impl ExecCompletion {
    pub fn exit_code(self) -> Option<i32> {
        match self {
            Self::CommandFinished { exit_code } | Self::ShellExited { exit_code } => exit_code,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::CommandFinished { .. } => "command_finished",
            Self::ShellExited { .. } => "shell_exited",
        }
    }
}

#[derive(Default)]
struct CommandCorrelation {
    expected: Option<String>,
    started: bool,
}

impl CommandCorrelation {
    fn observe(
        &mut self,
        event: &crate::agent::command_detection::CommandLifecycle,
    ) -> Option<ExecCompletion> {
        use crate::agent::command_detection::CommandLifecycle;
        match event {
            CommandLifecycle::Started { command_line, .. } => {
                self.started = self.expected.as_deref().map_or_else(
                    || command_line.as_deref().is_some_and(|line| !line.is_empty()),
                    |expected| {
                        command_line
                            .as_deref()
                            .is_some_and(|line| line.trim() == expected.trim())
                    },
                );
                None
            }
            CommandLifecycle::Finished { exit_code, .. } if std::mem::take(&mut self.started) => {
                Some(ExecCompletion::CommandFinished {
                    exit_code: *exit_code,
                })
            }
            _ => None,
        }
    }
}

struct PendingExec {
    id: u64,
    correlation: CommandCorrelation,
    tx: oneshot::Sender<ExecCompletion>,
    started: Arc<AtomicBool>,
}

type Waiters = HashMap<String, Vec<PendingExec>>;

static WAITERS: LazyLock<Mutex<Waiters>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_WAITER: AtomicU64 = AtomicU64::new(1);

pub struct ExecWaiter {
    pty_id: String,
    id: u64,
    rx: oneshot::Receiver<ExecCompletion>,
    started: Arc<AtomicBool>,
}

impl ExecWaiter {
    pub fn register(pty_id: &str) -> Self {
        Self::register_expected(pty_id, None)
    }

    pub fn for_command(pty_id: &str, command: &str) -> Self {
        Self::register_expected(pty_id, Some(command.to_string()))
    }

    fn register_expected(pty_id: &str, expected: Option<String>) -> Self {
        let id = NEXT_WAITER.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        let started = Arc::new(AtomicBool::new(false));
        WAITERS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(pty_id.to_string())
            .or_default()
            .push(PendingExec {
                id,
                correlation: CommandCorrelation {
                    expected,
                    started: false,
                },
                tx,
                started: started.clone(),
            });
        Self {
            pty_id: pty_id.to_string(),
            id,
            rx,
            started,
        }
    }

    pub(crate) fn started(&self) -> bool {
        self.started.load(Ordering::Acquire)
    }

    /// `None` when `deadline` elapsed first; elapsed time is a failure, never a completion.
    pub async fn wait(mut self, deadline: Duration) -> Option<ExecCompletion> {
        tokio::time::timeout(deadline, &mut self.rx)
            .await
            .ok()
            .and_then(Result::ok)
    }
}

impl Drop for ExecWaiter {
    fn drop(&mut self) {
        let mut waiters = WAITERS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(list) = waiters.get_mut(&self.pty_id) {
            list.retain(|pending| pending.id != self.id);
            if list.is_empty() {
                waiters.remove(&self.pty_id);
            }
        }
    }
}

pub fn publish(pty_id: &str, completion: ExecCompletion) {
    if !matches!(completion, ExecCompletion::ShellExited { .. }) {
        return;
    }
    let waiting = WAITERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(pty_id)
        .unwrap_or_default();
    for pending in waiting {
        let completion = if pending.correlation.expected.is_some() && !pending.correlation.started {
            ExecCompletion::ShellExited { exit_code: None }
        } else {
            completion
        };
        let _ = pending.tx.send(completion);
    }
}

pub(crate) fn observe_command(
    pty_id: &str,
    event: &crate::agent::command_detection::CommandLifecycle,
) {
    let mut waiters = WAITERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(list) = waiters.get_mut(pty_id) {
        let mut retained = Vec::new();
        for mut pending in list.drain(..) {
            let completion = pending.correlation.observe(event);
            if pending.correlation.started {
                pending.started.store(true, Ordering::Release);
            }
            if let Some(completion) = completion {
                let _ = pending.tx.send(completion);
            } else {
                retained.push(pending);
            }
        }
        *list = retained;
        if list.is_empty() {
            waiters.remove(pty_id);
        }
    }
    drop(waiters);
    crate::service::pty_io::submit_transaction::wake_submit_cycle(pty_id);
}

#[cfg(test)]
mod tests;
