use super::cycle::{SubmitCycleClaim, SubmitCycleSignal};
use crate::pty::write_queue::{GuardedInputPermit, WriteQueue};
use std::cell::Cell;
use std::sync::mpsc::SyncSender;
use std::time::Duration;

pub(super) struct MessagePayload {
    bytes: Vec<u8>,
    claim: SubmitCycleClaim,
    waiter: SyncSender<SubmitCycleSignal>,
}

#[derive(Debug)]
pub(super) enum PayloadWriteFailure {
    NotQueued(String),
    Queued(String),
}

pub(super) struct QueuedMessage {
    bytes: usize,
}

impl QueuedMessage {
    pub(super) fn len(&self) -> usize {
        self.bytes
    }
}

impl MessagePayload {
    pub(super) fn new(
        bytes: Vec<u8>,
        claim: SubmitCycleClaim,
        waiter: SyncSender<SubmitCycleSignal>,
    ) -> Self {
        Self {
            bytes,
            claim,
            waiter,
        }
    }

    pub(super) fn enqueue(
        self,
        queue: &WriteQueue,
        permit: Option<GuardedInputPermit>,
        timeout: Duration,
        written: impl FnOnce(),
    ) -> Result<QueuedMessage, PayloadWriteFailure> {
        let Self {
            bytes: payload,
            claim,
            waiter,
        } = self;
        let bytes = payload.len();
        let queued = Cell::new(false);
        let enqueued = || {
            queued.set(true);
            claim.payload_enqueued(waiter);
            written();
        };
        let result = match permit {
            Some(permit) => {
                queue.send_and_wait_guarded_with_enqueued(payload, permit, timeout, enqueued)
            }
            None => queue.send_and_wait_with_enqueued(payload, timeout, enqueued),
        };
        match result {
            Ok(()) => Ok(QueuedMessage { bytes }),
            Err(error) if crate::pty::write_queue::is_write_ack_timeout(&error) => {
                Ok(QueuedMessage { bytes })
            }
            Err(error) if queued.get() => Err(PayloadWriteFailure::Queued(error)),
            Err(error) => Err(PayloadWriteFailure::NotQueued(error)),
        }
    }
}

#[cfg(test)]
mod tests;
