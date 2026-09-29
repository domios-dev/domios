//! Completion evidence for `tab exec`: the shell-integration command-finished
//! marker, or the shell process itself exiting. A waiter registers before the
//! command is written, so no completion can land between the write and the wait.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use tokio::sync::oneshot;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExecCompletion {
    CommandFinished { exit_code: Option<i32> },
    ShellExited { exit_code: Option<i32> },
}

impl ExecCompletion {
    pub(crate) fn exit_code(self) -> Option<i32> {
        match self {
            Self::CommandFinished { exit_code } | Self::ShellExited { exit_code } => exit_code,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::CommandFinished { .. } => "command_finished",
            Self::ShellExited { .. } => "shell_exited",
        }
    }
}

type Waiters = HashMap<String, Vec<(u64, oneshot::Sender<ExecCompletion>)>>;

static WAITERS: LazyLock<Mutex<Waiters>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_WAITER: AtomicU64 = AtomicU64::new(1);

pub(crate) struct ExecWaiter {
    pty_id: String,
    id: u64,
    rx: oneshot::Receiver<ExecCompletion>,
}

impl ExecWaiter {
    pub(crate) fn register(pty_id: &str) -> Self {
        let id = NEXT_WAITER.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        WAITERS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(pty_id.to_string())
            .or_default()
            .push((id, tx));
        Self {
            pty_id: pty_id.to_string(),
            id,
            rx,
        }
    }

    /// `None` when `deadline` elapsed first; elapsed time is a failure, never a completion.
    pub(crate) async fn wait(mut self, deadline: Duration) -> Option<ExecCompletion> {
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
            list.retain(|(id, _)| *id != self.id);
            if list.is_empty() {
                waiters.remove(&self.pty_id);
            }
        }
    }
}

pub(crate) fn publish(pty_id: &str, completion: ExecCompletion) {
    let waiting = WAITERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(pty_id)
        .unwrap_or_default();
    for (_, tx) in waiting {
        let _ = tx.send(completion);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn exec_returns_exit_code_and_timeout() {
        let finished = ExecWaiter::register("pty-exec-finished");
        publish(
            "pty-exec-finished",
            ExecCompletion::CommandFinished { exit_code: Some(3) },
        );
        let completion = finished.wait(Duration::from_secs(30)).await;
        assert_eq!(completion.and_then(ExecCompletion::exit_code), Some(3));

        let exited = ExecWaiter::register("pty-exec-exited");
        publish(
            "pty-exec-exited",
            ExecCompletion::ShellExited { exit_code: Some(7) },
        );
        assert_eq!(
            exited.wait(Duration::from_secs(30)).await,
            Some(ExecCompletion::ShellExited { exit_code: Some(7) })
        );

        let silent = ExecWaiter::register("pty-exec-silent");
        assert_eq!(silent.wait(Duration::ZERO).await, None);
        assert!(!WAITERS.lock().unwrap().contains_key("pty-exec-silent"));

        publish(
            "pty-exec-late",
            ExecCompletion::CommandFinished { exit_code: Some(0) },
        );
        let late = ExecWaiter::register("pty-exec-late");
        assert_eq!(
            late.wait(Duration::ZERO).await,
            None,
            "a completion published before registration belongs to an earlier command"
        );
    }
}
