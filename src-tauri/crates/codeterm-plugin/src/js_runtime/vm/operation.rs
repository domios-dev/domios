use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use super::VmError;
use crate::converter::ConversionCancellation;

pub(in crate::js_runtime) const MAX_OPERATIONS: usize = 32;
const OPERATION_BOUND: Duration = super::super::call_deadline::STANDARD;

pub(in crate::js_runtime) struct Context {
    cancellation: ConversionCancellation,
    revoked: AtomicBool,
    effect_started: AtomicBool,
    jobs: AtomicUsize,
    unawaited: Mutex<HashSet<String>>,
    revoke_hooks: Mutex<Option<Vec<RevokeHook>>>,
    pub(in crate::js_runtime) deadline: Instant,
}

pub(in crate::js_runtime) type RevokeHook = Box<dyn FnOnce() + Send>;

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
            effect_started: AtomicBool::new(false),
            jobs: AtomicUsize::new(0),
            unawaited: Mutex::new(HashSet::new()),
            revoke_hooks: Mutex::new(Some(Vec::new())),
            deadline,
        }
    }

    pub(in crate::js_runtime) fn check(&self) -> Result<(), VmError> {
        if self.revoked.load(Ordering::Acquire) || self.cancellation.is_cancelled() {
            Err(if self.effect_started.load(Ordering::Acquire) {
                VmError::IoUnconfirmed
            } else {
                VmError::CallCancelled
            })
        } else if Instant::now() >= self.deadline {
            Err(if self.effect_started.load(Ordering::Acquire) {
                VmError::IoUnconfirmed
            } else {
                VmError::AsyncJobTimedOut
            })
        } else {
            Ok(())
        }
    }

    pub(in crate::js_runtime) fn begin_effect(&self) -> Result<(), VmError> {
        self.check()?;
        self.effect_started.store(true, Ordering::Release);
        Ok(())
    }

    pub(in crate::js_runtime) fn finish_check(&self) -> Result<(), VmError> {
        self.check()?;
        if self.jobs.load(Ordering::Acquire) != 0 || !self.unawaited.lock().is_empty() {
            return Err(if self.effect_started.load(Ordering::Acquire) {
                VmError::IoUnconfirmed
            } else {
                VmError::Script("host I/O job was not awaited".into())
            });
        }
        Ok(())
    }

    pub(in crate::js_runtime) fn register_job(&self, job: String) {
        self.unawaited.lock().insert(job);
    }
    pub(in crate::js_runtime) fn await_job(&self, job: &str) {
        self.unawaited.lock().remove(job);
    }

    pub(in crate::js_runtime) fn job(self: &Arc<Self>) -> Job {
        self.jobs.fetch_add(1, Ordering::AcqRel);
        Job(self.clone())
    }

    pub(in crate::js_runtime) fn cancellation(&self) -> &ConversionCancellation {
        &self.cancellation
    }

    pub(in crate::js_runtime) fn revoke(&self) {
        let hooks = {
            let mut hooks = self.revoke_hooks.lock();
            self.revoked.store(true, Ordering::Release);
            hooks.take()
        };
        self.cancellation.notify_change();
        for hook in hooks.into_iter().flatten() {
            hook();
        }
    }

    /// Runs `hook` once when this call is revoked, or now if it already was.
    pub(in crate::js_runtime) fn on_revoke(&self, hook: RevokeHook) {
        let mut hooks = self.revoke_hooks.lock();
        if let Some(pending) = hooks.as_mut() {
            pending.push(hook);
            return;
        }
        drop(hooks);
        hook();
    }
}

#[derive(Default)]
struct State {
    pauses: usize,
    active: HashMap<u64, Arc<Context>>,
}

#[derive(Default)]
pub(in crate::js_runtime) struct Admission {
    state: Mutex<State>,
    next_id: AtomicU64,
}

impl Admission {
    pub(in crate::js_runtime) fn enter(
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

    pub(in crate::js_runtime) fn pause(self: &Arc<Self>) -> Pause {
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

pub(in crate::js_runtime) struct Operation {
    admission: Arc<Admission>,
    id: u64,
    pub(in crate::js_runtime) context: Arc<Context>,
}

impl Drop for Operation {
    fn drop(&mut self) {
        self.context.revoke();
        self.admission.state.lock().active.remove(&self.id);
    }
}

pub(in crate::js_runtime) struct Pause(Arc<Admission>);

impl Drop for Pause {
    fn drop(&mut self) {
        self.0.state.lock().pauses -= 1;
    }
}

pub(in crate::js_runtime) struct Job(Arc<Context>);

impl Drop for Job {
    fn drop(&mut self) {
        self.0.jobs.fetch_sub(1, Ordering::AcqRel);
    }
}

thread_local! {
    static CURRENT: RefCell<Option<Arc<Context>>> = const { RefCell::new(None) };
}

pub(in crate::js_runtime) fn current() -> Option<Arc<Context>> {
    CURRENT.with(|slot| slot.borrow().clone())
}

pub(in crate::js_runtime) struct Scope(Option<Arc<Context>>);

impl Scope {
    pub(in crate::js_runtime) fn enter(context: Arc<Context>) -> Self {
        Self(CURRENT.with(|slot| slot.replace(Some(context))))
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        CURRENT.with(|slot| *slot.borrow_mut() = self.0.take());
    }
}

#[cfg(test)]
mod tests;
