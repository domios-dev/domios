use super::{InvocationContext, ProvisionError, State};
use crate::service::ServiceContext;
use parking_lot::{Condvar, Mutex};
use std::sync::{atomic::{AtomicBool, Ordering}, Arc};
use std::time::{Duration, Instant};

type ResultContext = Result<Option<Arc<InvocationContext>>, String>;
enum Provisioned {
    Invocation(Option<Arc<InvocationContext>>),
    Hook(Option<bool>),
    TypedInvocation(Result<Option<Arc<InvocationContext>>, ProvisionError>),
}
type JobResult = Result<Provisioned, String>;
const UNCONFIRMED: &str = "launch_account_supervision_unconfirmed: provisioning observation ended; retained work cannot authorize a launch";

thread_local! {
    static OWNED: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}
struct Scope(Option<usize>);
impl Drop for Scope {
    fn drop(&mut self) { OWNED.with(|owned| owned.set(self.0)); }
}

fn owns(svc: &ServiceContext) -> bool {
    let state = svc.ext_or_init::<State>(State::default) as *const State as usize;
    OWNED.with(|owned| owned.get() == Some(state))
}

pub(super) struct Owner {
    cancelled: AtomicBool,
    finished: AtomicBool,
    result: Mutex<Option<JobResult>>,
    changed: Condvar,
    notified: tokio::sync::Notify,
    handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

pub(crate) struct Observation(Arc<Owner>, AtomicBool);

struct Retained {
    service: ServiceContext,
    owner: Arc<Owner>,
    _receipt: super::work::Receipt,
}

impl Drop for Retained {
    fn drop(&mut self) {
        let mut slot = self.service.ext_or_init::<State>(State::default).provisioning_job.lock();
        if slot.as_ref().is_some_and(|owner| Arc::ptr_eq(owner, &self.owner)) {
            *slot = None;
        }
    }
}

pub(crate) fn start(
    svc: &ServiceContext,
    provision: impl FnOnce() -> ResultContext + Send + 'static,
) -> Result<Observation, String> {
    start_job(svc, move || provision().map(Provisioned::Invocation))
}

pub(super) fn invocation(svc: &ServiceContext, provision: impl FnOnce() -> ResultContext + Send + 'static) -> ResultContext {
    if owns(svc) { return provision(); }
    start(svc, provision)?.wait_blocking(Duration::from_secs(60))
}

pub(super) fn typed_invocation(
    svc: &ServiceContext,
    provision: impl FnOnce() -> Result<Option<Arc<InvocationContext>>, ProvisionError> + Send + 'static,
) -> Result<Option<Arc<InvocationContext>>, ProvisionError> {
    if owns(svc) { return provision(); }
    let observation = start_job(svc, move || Ok(Provisioned::TypedInvocation(provision())))?;
    match observation.wait_raw_blocking(Duration::from_secs(60))? {
        Provisioned::TypedInvocation(result) => result,
        _ => Err("account_provisioning_refused: unexpected job result".into()),
    }
}

pub(super) fn hook(
    svc: &ServiceContext, prepare: impl FnOnce() -> Result<Option<bool>, String> + Send + 'static,
) -> Result<Option<bool>, String> {
    if owns(svc) { return prepare(); }
    match start_job(svc, move || prepare().map(Provisioned::Hook))?.wait_raw_blocking(Duration::from_secs(60))? {
        Provisioned::Hook(available) => Ok(available),
        _ => Err("account_provisioning_refused: unexpected job result".into()),
    }
}

fn start_job(
    svc: &ServiceContext, provision: impl FnOnce() -> JobResult + Send + 'static,
) -> Result<Observation, String> {
    if crate::crash::shutdown::is_shutting_down() {
        return Err("account_provisioning_refused: service is shutting down".into());
    }
    let state = svc.ext_or_init::<State>(State::default);
    let owner = Arc::new(Owner {
        cancelled: AtomicBool::new(false), finished: AtomicBool::new(false), result: Mutex::new(None),
        changed: Condvar::new(), notified: tokio::sync::Notify::new(), handle: Mutex::new(None),
    });
    {
        let mut slot = state.provisioning_job.lock();
        if slot.is_some() {
            return Err("account_provisioning_busy: a retained provisioning job is still running".into());
        }
        *slot = Some(owner.clone());
    }
    let retained = Retained { service: svc.clone(), owner: owner.clone(), _receipt: super::work::Receipt::capture(svc) };
    let job = owner.clone();
    let identity = state as *const State as usize;
    let handle = svc.rt.spawn_blocking(move || {
        let _scope = Scope(OWNED.with(|owned| owned.replace(Some(identity))));
        let result = if job.cancelled.load(Ordering::Acquire) || crate::crash::shutdown::is_shutting_down() {
            Err(UNCONFIRMED.into())
        } else {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(provision))
                .unwrap_or_else(|_| Err("account_provisioning_refused: retained job exited unexpectedly".into()))
        };
        let result = if job.cancelled.load(Ordering::Acquire) || crate::crash::shutdown::is_shutting_down() {
            Err(UNCONFIRMED.into())
        } else { result };
        drop(retained);
        let mut stored = job.result.lock();
        *stored = Some(result);
        job.finished.store(true, Ordering::Release);
        job.changed.notify_all();
        job.notified.notify_waiters();
        drop(stored);
    });
    *owner.handle.lock() = Some(handle);
    Ok(Observation(owner, AtomicBool::new(false)))
}

impl Observation {
    pub(crate) fn observed_completed_refusal(&self) -> bool {
        let _result = self.0.result.lock();
        self.1.load(Ordering::Relaxed) && self.0.finished.load(Ordering::Acquire)
            && !self.0.cancelled.load(Ordering::Acquire)
    }

    fn take_settled(&self, result: &mut Option<JobResult>) -> Option<JobResult> {
        if self.0.cancelled.load(Ordering::Acquire) { return Some(Err(UNCONFIRMED.into())); }
        if !self.0.finished.load(Ordering::Acquire) { return None; }
        let value = result.take()?;
        self.1.store(value.is_err(), Ordering::Relaxed);
        Some(value)
    }
    pub(crate) fn wait_blocking(&self, timeout: Duration) -> ResultContext {
        match self.wait_raw_blocking(timeout)? {
            Provisioned::Invocation(invocation) => Ok(invocation),
            _ => Err("account_provisioning_refused: unexpected job result".into()),
        }
    }

    fn wait_raw_blocking(&self, timeout: Duration) -> JobResult {
        let deadline = Instant::now() + timeout;
        let mut result = self.0.result.lock();
        loop {
            if let Some(result) = self.take_settled(&mut result) { return result; }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                self.0.cancelled.store(true, Ordering::Release);
                return Err(UNCONFIRMED.into());
            }
            self.0.changed.wait_for(&mut result, remaining);
        }
    }

    pub(crate) async fn wait(&self, timeout: Duration) -> ResultContext {
        let result = tokio::time::timeout(timeout, async {
            loop {
                let changed = self.0.notified.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                let result = self.take_settled(&mut self.0.result.lock());
                if let Some(result) = result { return result; }
                changed.await;
            }
        }).await;
        match result.unwrap_or_else(|_| {
            let _result = self.0.result.lock();
            self.0.cancelled.store(true, Ordering::Release);
            Err(UNCONFIRMED.into())
        })? {
            Provisioned::Invocation(invocation) => Ok(invocation),
            _ => Err("account_provisioning_refused: unexpected job result".into()),
        }
    }
}

impl Drop for Observation {
    fn drop(&mut self) {
        let _result = self.0.result.lock();
        self.0.cancelled.store(true, Ordering::Release);
    }
}

#[cfg(test)]
pub(crate) fn retained_count(svc: &ServiceContext) -> usize {
    usize::from(svc.ext_or_init::<State>(State::default).provisioning_job.lock().is_some())
}
