use codeterm_neighbors::{Error, Result};
use std::sync::atomic::{AtomicU64, Ordering};

/// An entitlement refusal cannot change until an entitlement or account event, so it waits for one.
pub(in crate::service::neighbors) fn entitlement_refusal(error: Error) -> bool {
    matches!(error, Error::Locked)
}

#[derive(Debug, PartialEq, Eq)]
pub(in crate::service::neighbors) enum HoldTransition {
    Held(Error),
    Released(Error),
    Unchanged,
}

#[derive(Default)]
pub(in crate::service::neighbors) struct EntitlementHold {
    epoch: AtomicU64,
    changed: tokio::sync::Notify,
    state: parking_lot::Mutex<HoldState>,
}

#[derive(Default)]
struct HoldState {
    held_at: Option<u64>,
    reason: Option<Error>,
}

impl EntitlementHold {
    pub fn changed(&self) {
        self.epoch.fetch_add(1, Ordering::AcqRel);
        self.changed.notify_waiters();
    }

    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }

    pub async fn released(&self) {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.state.lock().held_at != Some(self.epoch()) {
                return;
            }
            changed.await;
        }
    }

    /// Records an attempt that began at `epoch`; only a change of reason is a transition to log.
    pub fn settle<T>(&self, epoch: u64, result: &Result<T>) -> HoldTransition {
        let mut state = self.state.lock();
        match result {
            Err(error) if entitlement_refusal(*error) => {
                state.held_at = Some(epoch);
                if state.reason.replace(*error) == Some(*error) {
                    HoldTransition::Unchanged
                } else {
                    HoldTransition::Held(*error)
                }
            }
            _ => {
                state.held_at = None;
                state
                    .reason
                    .take()
                    .map_or(HoldTransition::Unchanged, HoldTransition::Released)
            }
        }
    }
}
