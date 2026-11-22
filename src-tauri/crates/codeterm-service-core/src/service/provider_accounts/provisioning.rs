use super::{InvocationContext, ProvisionError, State};
use crate::multiplexer::types::Tab;
use crate::service::messenger::switch_handoff::same_binding;
use crate::service::restore_admission::{changes, priority};
use crate::service::ServiceContext;
use parking_lot::{Condvar, Mutex};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

type ResultContext = Result<Option<Arc<InvocationContext>>, String>;
enum Provisioned {
    Invocation(Option<Arc<InvocationContext>>),
    Hook(Option<bool>),
    TypedInvocation(Result<Option<Arc<InvocationContext>>, ProvisionError>),
}
type JobResult = Result<Provisioned, String>;
const UNCONFIRMED: &str = "launch_account_supervision_unconfirmed: provisioning observation ended; retained work cannot authorize a launch";
const SHUTDOWN: &str = "account_provisioning_refused: service is shutting down";
const BUSY: &str = "account_provisioning_busy: a retained provisioning job is still running";
const SUPERSEDED: &str = "launch_account_source_changed: startup provisioning authority changed";

thread_local! {
    static OWNED: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
    static STARTUP: std::cell::RefCell<Option<(usize, String)>> = const { std::cell::RefCell::new(None) };
}
struct Scope(Option<usize>);
impl Drop for Scope {
    fn drop(&mut self) {
        OWNED.with(|owned| owned.set(self.0));
    }
}

fn owns(svc: &ServiceContext) -> bool {
    let state = identity(svc);
    OWNED.with(|owned| owned.get() == Some(state))
}

fn identity(svc: &ServiceContext) -> usize {
    svc.ext_or_init::<State>(State::default) as *const State as usize
}

#[must_use]
pub(crate) struct StartupScope(
    Option<(usize, String)>,
    std::marker::PhantomData<std::rc::Rc<()>>,
);

impl StartupScope {
    pub(crate) fn enter(svc: &ServiceContext, tab: &str) -> Self {
        Self(
            STARTUP.with(|scope| scope.replace(Some((identity(svc), tab.to_owned())))),
            std::marker::PhantomData,
        )
    }
}

impl Drop for StartupScope {
    fn drop(&mut self) {
        STARTUP.with(|scope| scope.replace(self.0.take()));
    }
}

fn startup_tab(svc: &ServiceContext) -> Option<String> {
    STARTUP.with(|scope| {
        scope
            .borrow()
            .as_ref()
            .filter(|(state, _)| *state == identity(svc))
            .map(|(_, tab)| tab.clone())
    })
}

pub(crate) fn in_startup(svc: &ServiceContext, tab: &str) -> bool {
    startup_tab(svc).as_deref() == Some(tab)
}

pub(crate) fn startup_tab_is_current(svc: &ServiceContext, expected: &Tab) -> bool {
    svc.mux.read().get_tab(&expected.id).is_some_and(|current| {
        same_binding(expected, current)
            && current.restored_from_snapshot == expected.restored_from_snapshot
            && !current.frozen
            && current.boot_stage != Some(crate::service::boot_stage::BootStage::SpawnFailed)
            && !current
                .close_restore
                .as_ref()
                .is_some_and(|closed| closed.stopped)
    })
}

fn validate_startup_tab(svc: &ServiceContext, expected: &Tab) -> Result<(), String> {
    if startup_tab_is_current(svc, expected) {
        Ok(())
    } else {
        Err(SUPERSEDED.into())
    }
}

struct StartupWaiter<'a> {
    state: &'a State,
    tab: Arc<str>,
}

impl Drop for StartupWaiter<'_> {
    fn drop(&mut self) {
        self.state
            .startup_waiters
            .lock()
            .retain(|tab| !Arc::ptr_eq(tab, &self.tab));
        changes().notify();
    }
}

fn reserve_startup(
    svc: &ServiceContext,
    state: &State,
    expected: &Tab,
    owner: &Arc<Owner>,
) -> Result<(), String> {
    let tab = Arc::<str>::from(expected.id.as_str());
    state.startup_waiters.lock().push(tab.clone());
    let _waiter = StartupWaiter {
        state,
        tab: tab.clone(),
    };
    changes().notify();
    crate::startup::boot_timing::mark(&expected.id, None, "startup_provisioning_queued");
    loop {
        let observed = changes().revision();
        if crate::crash::shutdown::is_shutting_down() {
            return Err(SHUTDOWN.into());
        }
        validate_startup_tab(svc, expected)?;
        {
            let waiting = state.startup_waiters.lock();
            let mut slot = state.provisioning_job.lock();
            if slot.is_none() {
                let mux = svc.mux.read();
                let next = waiting.iter().min_by_key(|tab| priority(&mux, tab));
                if next.is_some_and(|next| Arc::ptr_eq(next, &tab)) {
                    return crate::crash::shutdown::with_shutdown_admission(|| {
                        *slot = Some(owner.clone());
                    })
                    .ok_or_else(|| SHUTDOWN.into());
                }
            }
        }
        changes().wait(observed);
    }
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
        let mut slot = self
            .service
            .ext_or_init::<State>(State::default)
            .provisioning_job
            .lock();
        if slot
            .as_ref()
            .is_some_and(|owner| Arc::ptr_eq(owner, &self.owner))
        {
            *slot = None;
            drop(slot);
            changes().notify();
        }
    }
}

pub(super) fn require_idle(svc: &ServiceContext) -> Result<(), String> {
    if crate::crash::shutdown::is_shutting_down() {
        return Err(SHUTDOWN.into());
    }
    if !owns(svc)
        && startup_tab(svc).is_none()
        && svc
            .ext_or_init::<State>(State::default)
            .provisioning_job
            .lock()
            .is_some()
    {
        return Err(BUSY.into());
    }
    Ok(())
}

pub(crate) fn start(
    svc: &ServiceContext,
    provision: impl FnOnce() -> ResultContext + Send + 'static,
) -> Result<Observation, String> {
    start_job(svc, move || provision().map(Provisioned::Invocation))
}

pub(super) fn invocation(
    svc: &ServiceContext,
    provision: impl FnOnce() -> ResultContext + Send + 'static,
) -> ResultContext {
    if owns(svc) {
        return provision();
    }
    start(svc, provision)?.wait_blocking(Duration::from_secs(60))
}

pub(super) fn typed_invocation(
    svc: &ServiceContext,
    provision: impl FnOnce() -> Result<Option<Arc<InvocationContext>>, ProvisionError> + Send + 'static,
) -> Result<Option<Arc<InvocationContext>>, ProvisionError> {
    if owns(svc) {
        return provision();
    }
    let observation = start_job(svc, move || Ok(Provisioned::TypedInvocation(provision())))?;
    match observation.wait_raw_blocking(Duration::from_secs(60))? {
        Provisioned::TypedInvocation(result) => result,
        _ => Err("account_provisioning_refused: unexpected job result".into()),
    }
}

pub(super) fn hook(
    svc: &ServiceContext,
    prepare: impl FnOnce() -> Result<Option<bool>, String> + Send + 'static,
) -> Result<Option<bool>, String> {
    if owns(svc) {
        return prepare();
    }
    match start_job(svc, move || prepare().map(Provisioned::Hook))?
        .wait_raw_blocking(Duration::from_secs(60))?
    {
        Provisioned::Hook(available) => Ok(available),
        _ => Err("account_provisioning_refused: unexpected job result".into()),
    }
}

fn start_job(
    svc: &ServiceContext,
    provision: impl FnOnce() -> JobResult + Send + 'static,
) -> Result<Observation, String> {
    if crate::crash::shutdown::is_shutting_down() {
        return Err(SHUTDOWN.into());
    }
    let state = svc.ext_or_init::<State>(State::default);
    let owner = Arc::new(Owner {
        cancelled: AtomicBool::new(false),
        finished: AtomicBool::new(false),
        result: Mutex::new(None),
        changed: Condvar::new(),
        notified: tokio::sync::Notify::new(),
        handle: Mutex::new(None),
    });
    let expected = startup_tab(svc)
        .map(|tab| svc.mux.read().get_tab(&tab).cloned().ok_or(SUPERSEDED))
        .transpose()?;
    if let Some(expected) = &expected {
        reserve_startup(svc, state, expected, &owner)?;
        crate::startup::boot_timing::mark(&expected.id, None, "startup_provisioning_granted");
    } else {
        let waiters = state.startup_waiters.lock();
        let mut slot = state.provisioning_job.lock();
        if slot.is_some() || !waiters.is_empty() {
            return Err(BUSY.into());
        }
        crate::crash::shutdown::with_shutdown_admission(|| {
            *slot = Some(owner.clone());
        })
        .ok_or(SHUTDOWN)?;
    }
    let retained = Retained {
        service: svc.clone(),
        owner: owner.clone(),
        _receipt: super::work::Receipt::capture(svc),
    };
    let job = owner.clone();
    let identity = identity(svc);
    let service = svc.clone();
    let handle = svc.rt.spawn_blocking(move || {
        let _scope = Scope(OWNED.with(|owned| owned.replace(Some(identity))));
        let result = if job.cancelled.load(Ordering::Acquire)
            || crate::crash::shutdown::is_shutting_down()
        {
            Err(UNCONFIRMED.into())
        } else {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if let Some(expected) = &expected {
                    validate_startup_tab(&service, expected)?;
                }
                provision()
            }))
            .unwrap_or_else(|_| {
                Err("account_provisioning_refused: retained job exited unexpectedly".into())
            })
        };
        let result = if job.cancelled.load(Ordering::Acquire)
            || crate::crash::shutdown::is_shutting_down()
        {
            Err(UNCONFIRMED.into())
        } else {
            result
        };
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
        self.1.load(Ordering::Relaxed)
            && self.0.finished.load(Ordering::Acquire)
            && !self.0.cancelled.load(Ordering::Acquire)
    }

    fn take_settled(&self, result: &mut Option<JobResult>) -> Option<JobResult> {
        if self.0.cancelled.load(Ordering::Acquire) {
            return Some(Err(UNCONFIRMED.into()));
        }
        if !self.0.finished.load(Ordering::Acquire) {
            return None;
        }
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
            if let Some(result) = self.take_settled(&mut result) {
                return result;
            }
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
                if let Some(result) = result {
                    return result;
                }
                changed.await;
            }
        })
        .await;
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
    usize::from(
        svc.ext_or_init::<State>(State::default)
            .provisioning_job
            .lock()
            .is_some(),
    )
}

#[cfg(test)]
pub(crate) fn wait_for_startup_waiters(svc: &ServiceContext, count: usize, deadline: Instant) {
    let state = svc.ext_or_init::<State>(State::default);
    loop {
        let observed = changes().revision();
        if state.startup_waiters.lock().len() == count {
            return;
        }
        assert!(
            changes().wait_until(observed, deadline),
            "startup waiters did not register"
        );
    }
}
