use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use super::VmError;
use crate::converter::ConversionCancellation;

const MAX_OPERATIONS: usize = 32;
const OPERATION_BOUND: Duration = Duration::from_secs(5);
const FRAME_BOUND: Duration = Duration::from_millis(250);

pub(super) struct Context {
    cancellation: ConversionCancellation,
    revoked: AtomicBool,
    pub(super) deadline: Instant,
}

impl Context {
    fn new(interrupt: Option<(ConversionCancellation, Instant)>) -> Self {
        let (cancellation, deadline) = interrupt.unwrap_or_else(|| {
            (
                ConversionCancellation::default(),
                Instant::now() + OPERATION_BOUND,
            )
        });
        let deadline =
            codeterm_decision::physical::deadline().map_or(deadline, |outer| outer.min(deadline));
        Self {
            cancellation,
            revoked: AtomicBool::new(false),
            deadline,
        }
    }

    pub(super) fn check(&self) -> Result<(), VmError> {
        if self.revoked.load(Ordering::Acquire) || self.cancellation.is_cancelled() {
            Err(VmError::CallCancelled)
        } else if Instant::now() >= self.deadline {
            Err(VmError::AsyncJobTimedOut)
        } else {
            Ok(())
        }
    }

    pub(super) fn frame_deadline(&self) -> Instant {
        self.deadline.min(Instant::now() + FRAME_BOUND)
    }

    pub(super) fn cancellation(&self) -> &ConversionCancellation {
        &self.cancellation
    }

    pub(super) fn revoke(&self) {
        self.revoked.store(true, Ordering::Release);
        self.cancellation.notify_change();
    }
}

#[derive(Default)]
struct State {
    pauses: usize,
    active: HashMap<u64, Arc<Context>>,
}

#[derive(Default)]
pub(super) struct Admission {
    state: Mutex<State>,
    next_id: AtomicU64,
}

impl Admission {
    pub(super) fn enter(
        self: &Arc<Self>,
        interrupt: Option<(ConversionCancellation, Instant)>,
    ) -> Result<Operation, VmError> {
        let context = Arc::new(Context::new(interrupt));
        context.check()?;
        let mut state = self.state.lock();
        if state.pauses != 0 || state.active.len() >= MAX_OPERATIONS {
            return Err(VmError::AdmissionBusy);
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        state.active.insert(id, context.clone());
        Ok(Operation {
            admission: self.clone(),
            id,
            context,
        })
    }

    pub(super) fn pause(self: &Arc<Self>) -> Pause {
        let mut state = self.state.lock();
        state.pauses += 1;
        let active = state.active.values().cloned().collect::<Vec<_>>();
        drop(state);
        for context in active {
            context.revoke();
        }
        Pause(self.clone())
    }
}

pub(super) struct Operation {
    admission: Arc<Admission>,
    id: u64,
    pub(super) context: Arc<Context>,
}

impl Drop for Operation {
    fn drop(&mut self) {
        self.context.revoke();
        self.admission.state.lock().active.remove(&self.id);
    }
}

pub(super) struct Pause(Arc<Admission>);

impl Drop for Pause {
    fn drop(&mut self) {
        self.0.state.lock().pauses -= 1;
    }
}

#[cfg(test)]
mod tests;
