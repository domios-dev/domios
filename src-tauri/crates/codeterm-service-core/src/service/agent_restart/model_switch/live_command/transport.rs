use std::cell::Cell;
use std::time::Duration;

use crate::pty::write_queue::{GuardedInputPermit, WriteQueue};

pub(super) fn enqueue_command(
    queue: &WriteQueue,
    permit: GuardedInputPermit,
    input: Vec<u8>,
    timeout: Duration,
    injected: &Cell<bool>,
) -> Result<(), String> {
    enqueue_command_observed(queue, permit, input, timeout, injected, || {})
}

pub(super) fn enqueue_command_observed(
    queue: &WriteQueue,
    permit: GuardedInputPermit,
    input: Vec<u8>,
    timeout: Duration,
    injected: &Cell<bool>,
    on_enqueued: impl FnOnce(),
) -> Result<(), String> {
    let written = queue.send_and_wait_guarded_with_enqueued(input, permit, timeout, || {
        injected.set(true);
        on_enqueued();
    });
    if let Err(error) = written {
        if crate::pty::write_queue::is_stale_agent_input(&error) || !injected.get() {
            injected.set(false);
            return Err(error);
        }
        // Queued command uncertainty is resolved only by its declared own-session output.
        log::warn!("[agent-model] command write acknowledgement uncertain: {error}");
    }
    Ok(())
}

pub(super) fn finish_command(
    result: Result<(), String>,
    injected: bool,
    cleanup: impl FnOnce() -> Result<(), String>,
    rearm: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    if !injected {
        return result;
    }
    if let Err(error) = &result {
        cleanup().map_err(|cleanup| format!(
            "{error}; cleanup failed: {cleanup}; restart and resume is available as an explicit fallback"
        ))?;
    }
    rearm()?;
    result
}

#[cfg(test)]
mod tests;
