use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;
use std::sync::{atomic::Ordering, Arc};
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::json;
use tauri::Manager;

use super::{
    classify, machine, probe,
    provenance::{Evidence, Scope, Stage},
    routes, source, storage,
    store::{Lease, Sentinel, Store},
    wake,
};
mod advance;
pub(super) use advance::advance;
mod events;
mod probe_failure;
mod record;
pub use record::record;
#[cfg(test)]
pub(super) use record::record_unconfirmed_failure_with;
pub(super) use record::resolve_probe_result;
use record::{record_failure, record_input_unavailable, record_unconfirmed_failure};
pub mod text;

use codeterm_service_core::cron::clock::SharedClock;
use codeterm_service_core::service::{supporter, ServiceState};

const FAILURE_DEADLINE_MS: u64 = 60_000;

#[derive(Debug)]
struct KnownJournalRefusal(&'static str);

impl fmt::Display for KnownJournalRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for KnownJournalRefusal {}

pub(super) fn known_pre_effect_journal_refusal(message: &'static str) -> std::io::Error {
    std::io::Error::other(KnownJournalRefusal(message))
}

fn journal_refusal_prevented_effect(error: &std::io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|source| source.is::<KnownJournalRefusal>())
}

#[derive(Clone)]
pub struct Handle {
    pub store: Arc<Mutex<Store>>,
    pub(super) inbox: Arc<Mutex<super::terminal::Inbox>>,
    pending_run_bindings: Arc<Mutex<BTreeMap<String, super::run_binding::PendingRunBinding>>>,
    wake: Arc<(std::sync::Mutex<u64>, std::sync::Condvar)>,
    pub(super) probe_owner: Arc<probe::owner::ProbeResourceOwner>,
    app: Option<codeterm_service_core::app_runtime::AppHandle>,
    _shutdown: Option<Arc<codeterm_service_core::crash::shutdown::ShutdownSubscription>>,
}

impl Handle {
    pub(crate) fn cancel_tab_owned_runs(&self, tab_id: &str) -> usize {
        self.store.lock().cancel_tab_owned_runs(tab_id)
    }

    pub(crate) fn remove_worker_watchers(&self, tab_id: &str) -> Result<(), String> {
        let result = super::cleanup::remove_owned(&self.store, tab_id);
        self.notify();
        result
    }

    pub fn notify(&self) {
        if let Some(app) = &self.app {
            emit_changes(&self.store, app);
        }
        let (generation, wake) = &*self.wake;
        if let Ok(mut generation) = generation.lock() {
            *generation = generation.wrapping_add(1);
            wake.notify_all();
        }
    }

    pub(crate) fn wake_tab(&self, tab_id: &str) {
        let changed = self.store.lock().wake_tab(tab_id);
        if changed {
            self.notify();
        }
    }

    pub(super) fn queue_run_binding(
        &self,
        sentinel_id: String,
        binding: super::run_binding::PendingRunBinding,
    ) -> bool {
        let store = self.store.lock();
        let live_ids: BTreeSet<_> = store.list().iter().map(|job| job.id.clone()).collect();
        if !live_ids.contains(&sentinel_id) {
            return false;
        }
        let mut pending = self.pending_run_bindings.lock();
        pending.retain(|id, _| live_ids.contains(id));
        if let Some(existing) = pending.get(&sentinel_id) {
            if existing.started_at > binding.started_at {
                return false;
            }
        } else if pending.len() >= super::store::MAX_SENTINELS {
            return false;
        }
        pending.insert(sentinel_id, binding);
        true
    }

    pub(super) fn take_run_bindings(&self) -> Vec<(String, super::run_binding::PendingRunBinding)> {
        std::mem::take(&mut *self.pending_run_bindings.lock())
            .into_iter()
            .collect()
    }

    /// Blocking settlement: callers on the async runtime must move this off it.
    pub fn remove_team(&self, team: &str) -> Result<(), String> {
        let result = storage::commit(&self.store, |store| store.stage_remove_team(team));
        self.notify();
        result
    }

    pub(crate) fn remove_workspace(&self, workspace: &str) -> Result<(), String> {
        let jobs: Vec<_> = self
            .store
            .lock()
            .list()
            .iter()
            .filter(|job| job.workspace_id == workspace)
            .cloned()
            .collect();
        for job in jobs {
            if self.store.lock().get(&job.id).is_none() {
                continue;
            }
            storage::commit(&self.store, |store| {
                store.stage_delete_scenario(&job.id, workspace, job.team_id.as_deref())
            })?;
        }
        self.notify();
        Ok(())
    }
}

#[cfg(any(test, feature = "test-support"))]
pub fn fixture_handle(store: Store, app: codeterm_service_core::app_runtime::AppHandle) -> Handle {
    Handle {
        store: Arc::new(Mutex::new(store)),
        inbox: Arc::new(Mutex::new(super::terminal::Inbox::default())),
        pending_run_bindings: Arc::new(Mutex::new(BTreeMap::new())),
        wake: Arc::new((std::sync::Mutex::new(0), std::sync::Condvar::new())),
        probe_owner: Arc::new(probe::owner::ProbeResourceOwner::new()),
        app: Some(app),
        _shutdown: None,
    }
}

pub(super) fn emit_changes(
    store: &Arc<Mutex<Store>>,
    app: &codeterm_service_core::app_runtime::AppHandle,
) {
    let changes = store.lock().take_changes();
    for change in changes {
        codeterm_events::emit_everywhere(app, "automation-graph-changed", change);
    }
}

pub fn start(
    svc: ServiceState,
    clock: SharedClock,
    storage: &codeterm_service_core::storage::Storage,
) -> Option<codeterm_service_core::ports::sentinel::SentinelPorts> {
    let _ = super::command::START_TRACE.set(|id| {
        codeterm_service_core::flow_trace::record(id, "blueprint.started", "", "", "command", None);
    });
    let mut handle = Handle {
        store: Arc::new(Mutex::new(Store::with_storage(storage, clock))),
        inbox: Arc::new(Mutex::new(super::terminal::Inbox::default())),
        pending_run_bindings: Arc::new(Mutex::new(BTreeMap::new())),
        wake: Arc::new((std::sync::Mutex::new(0), std::sync::Condvar::new())),
        probe_owner: Arc::new(probe::owner::ProbeResourceOwner::new()),
        app: Some(svc.app_handle.clone()),
        _shutdown: None,
    };
    let callback = shutdown_callback(&handle);
    match codeterm_service_core::crash::shutdown::subscribe_sentinel_shutdown(callback) {
        Ok(subscription) => handle._shutdown = Some(Arc::new(subscription)),
        Err(error) => {
            log::error!("[blueprint] shutdown registration failed: {error}");
            handle.store.lock().close_admission();
            handle.probe_owner.close_admission();
            return None;
        }
    }
    crate::service_adapter::graph::install_invalidations(&svc, handle.store.clone());
    wake::listen_any(svc.clone(), handle.clone());
    svc.app_handle.manage(handle.clone());
    let ports = super::ports_impl::ports(&handle);
    svc.app_handle.manage(ports.clone());
    if let Err(error) = handle.store.lock().ensure_available() {
        log::error!("[blueprint] subsystem unavailable: {error}");
        return Some(ports);
    }
    if let Err(error) = super::input::sweep_orphans(&codeterm_service_core::paths::data_dir()) {
        log::error!("[blueprint-input] startup orphan sweep failed: {error}");
    }
    let store = handle.store.clone();
    let inbox = handle.inbox.clone();
    let pending_run_bindings = handle.pending_run_bindings.clone();
    let wake = handle.wake.clone();
    let probe_owner = Arc::downgrade(&handle.probe_owner);
    drop(handle);
    codeterm_util::named_thread::spawn("blueprint-scheduler", move || {
        let (generation, wake_condvar) = &*wake;
        let mut unavailable_incident: Option<String> = None;
        loop {
            let Some(owner) = probe_owner.upgrade() else {
                return;
            };
            if owner.is_closing() {
                return;
            }
            let observed = match generation.lock() {
                Ok(guard) => *guard,
                Err(_) => return,
            };
            let reconciliation = storage::reconcile(&store);
            let unavailable = store.lock().ensure_available().err();
            if let Some(error) = &unavailable {
                if unavailable_incident.as_ref() != Some(error) {
                    log::error!("[blueprint] scheduler unavailable: {error}");
                    unavailable_incident = Some(error.clone());
                }
            } else {
                unavailable_incident = None;
                if let Err(error) = reconciliation {
                    log::error!("[blueprint] unconfirmed commit reconciliation failed: {error}");
                }
                let run_handle = Handle {
                    store: store.clone(),
                    inbox: inbox.clone(),
                    pending_run_bindings: pending_run_bindings.clone(),
                    wake: wake.clone(),
                    probe_owner: owner.clone(),
                    app: Some(svc.app_handle.clone()),
                    _shutdown: None,
                };
                super::run_binding::process_pending(&svc, &run_handle);
                super::run_binding::reconcile_runs(&svc, &run_handle);
                expire_escalations(&store, &svc);
                run_handle.reconcile_terminal_sources(&svc);
                run_handle.prepare_text_events();
                match storage::commit(&store, Store::stage_due) {
                    Ok(leases) => {
                        let notices = store.lock().take_stop_notices();
                        for notice in notices {
                            routes::notify_stop(&svc, &notice);
                        }
                        for lease in leases {
                            let Some(owner) = probe_owner.upgrade() else {
                                return;
                            };
                            let worker_handle = Handle {
                                store: store.clone(),
                                inbox: inbox.clone(),
                                pending_run_bindings: pending_run_bindings.clone(),
                                wake: wake.clone(),
                                probe_owner: owner,
                                app: Some(svc.app_handle.clone()),
                                _shutdown: None,
                            };
                            if worker_handle.probe_owner.is_closing() {
                                return;
                            }
                            let svc = svc.clone();
                            std::thread::spawn(move || {
                                tick(&worker_handle, &svc, &lease);
                                worker_handle.store.lock().release(&lease.job.id);
                                worker_handle.prepare_text_events();
                                worker_handle.notify();
                            });
                        }
                        events::reconcile_subscriptions(&svc, &run_handle);
                        events::dispatch_runs(&svc, &run_handle);
                    }
                    Err(error) => {
                        if let Some(unavailable) = store.lock().ensure_available().err() {
                            if unavailable_incident.as_ref() != Some(&unavailable) {
                                log::error!("[blueprint] scheduler unavailable: {unavailable}");
                                unavailable_incident = Some(unavailable);
                            }
                        } else {
                            log::error!("[blueprint] schedule persistence failed: {error}");
                        }
                    }
                }
                if let Err(error) = super::cleanup::remove_terminal(&store) {
                    log::error!("[blueprint] terminal watcher cleanup failed: {error}");
                }
            }
            emit_changes(&store, &svc.app_handle);
            if probe_owner.upgrade().is_none_or(|owner| owner.is_closing()) {
                return;
            }
            let now = store.lock().clock.now_ms();
            let terminal_deadline = inbox.lock().deadline();
            let wait_ms = scheduler_wait_ms(&store.lock())
                .min(terminal_deadline.map_or(u64::MAX, |at| at.saturating_sub(now).max(1)));
            let event_wait_only = store.lock().event_wait_only();
            let guard = match generation.lock() {
                Ok(guard) => guard,
                Err(_) => return,
            };
            if *guard != observed {
                continue;
            }
            if event_wait_only && terminal_deadline.is_none() {
                drop(wake_condvar.wait_while(guard, |value| *value == observed));
            } else {
                let _ = wake_condvar.wait_timeout_while(
                    guard,
                    Duration::from_millis(wait_ms),
                    |value| *value == observed,
                );
            }
        }
    });
    Some(ports)
}

fn scheduler_wait_ms(store: &Store) -> u64 {
    if store.ensure_available().is_err() {
        FAILURE_DEADLINE_MS
    } else {
        store.next_deadline().max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Create, Owner};
    use serde_json::json;

    #[test]
    fn blueprint_reason_every_transition_reason_has_one_route_classification() {
        for reason in [
            machine::TransitionReason::Transition,
            machine::TransitionReason::RearmOnChange,
            machine::TransitionReason::Other,
            machine::TransitionReason::LowConfidence,
        ] {
            assert!(
                machine::TransitionReason::parse(reason.as_str())
                    .is_some_and(machine::TransitionReason::routes_handlers),
                "{} must keep its route claim",
                reason.as_str()
            );
        }
        for reason in [
            machine::TransitionReason::SameBucket,
            machine::TransitionReason::Paused,
            machine::TransitionReason::Stopped,
            machine::TransitionReason::Held,
            machine::TransitionReason::Unchanged,
        ] {
            assert!(!machine::TransitionReason::parse(reason.as_str())
                .is_some_and(machine::TransitionReason::routes_handlers));
        }
    }

    #[test]
    fn sentinel_unavailable_wait_uses_failure_deadline_for_overdue_jobs() {
        let temp = tempfile::tempdir().unwrap();
        let clock = codeterm_service_core::cron::clock::MockClock::starting_at(1_000);
        let mut store = Store::new(temp.path().join("sentinels.json"), clock.clone());
        let params: Create = serde_json::from_value(json!({
            "name": "scheduler-readiness-watch",
            "probe": "status",
            "question": "ready?",
            "buckets": ["done"],
            "every": "10s",
            "timeout": "1h",
            "matches": {"done": "done"},
            "handlers": {"done": [{"id": "notice", "type": "notify", "parameters": {}}]}
        }))
        .unwrap();
        let job = store
            .create(
                params,
                Owner {
                    creator_tab_id: "creator".into(),
                    team_id: None,
                    workspace_id: "workspace".into(),
                    cwd: ".".into(),
                },
                false,
            )
            .unwrap();
        clock.set_ms(job.next_run);
        store.unavailable = Some(storage::LoadFailure::PersistenceFailed);

        assert_eq!(store.next_deadline(), 0);
        assert_eq!(scheduler_wait_ms(&store), FAILURE_DEADLINE_MS);
    }
}

fn shutdown_callback(handle: &Handle) -> impl FnOnce() + Send + 'static {
    let store = Arc::downgrade(&handle.store);
    let owner = Arc::downgrade(&handle.probe_owner);
    let wake = Arc::downgrade(&handle.wake);
    move || {
        if let Some(store) = store.upgrade() {
            store.lock().close_admission();
        }
        if let Some(owner) = owner.upgrade() {
            owner.close_admission();
        }
        if let Some(wake) = wake.upgrade() {
            let (generation, condvar) = &*wake;
            if let Ok(mut generation) = generation.lock() {
                *generation = generation.wrapping_add(1);
                condvar.notify_all();
            }
        }
    }
}

#[cfg(test)]
pub(super) fn fixture_shutdown(
    handle: &Handle,
    latch: &codeterm_shutdown::ShutdownLatch,
) -> codeterm_service_core::crash::shutdown::ShutdownSubscription {
    latch.subscribe(shutdown_callback(handle)).unwrap()
}

fn journal(
    svc: &ServiceState,
    job: &Sentinel,
    outcome: &machine::Outcome,
    stage: Stage,
) -> std::io::Result<()> {
    record(
        &supporter::runtime_with_notifications(svc),
        job,
        outcome,
        stage,
    )
}

fn expire_escalations(store: &Arc<Mutex<Store>>, svc: &ServiceState) {
    loop {
        let staged = { store.lock().stage_expire_escalation() };
        let Some((job, staged)) = staged else { return };
        match staged.settle(store) {
            Ok(true) => {
                let Some(transition_id) = job.machine.held.clone() else {
                    return;
                };
                let mut evidence = Evidence::new(Scope::from(&job), job.machine.last_hash.clone());
                evidence.evaluation_id.clone_from(&transition_id);
                evidence.observation_id = Some(transition_id);
                let outcome = machine::Outcome {
                    evidence,
                    reason: "escalation_expired",
                    verdict: None,
                    route: None,
                    handlers: Vec::new(),
                };
                if let Err(error) = journal(svc, &job, &outcome, Stage::EscalationExpired) {
                    log::error!("[blueprint] escalation expiry journal failed: {error}");
                }
            }
            Ok(false) => continue,
            Err(error) => {
                log::error!("[blueprint] escalation expiry persistence failed: {error}");
                return;
            }
        }
    }
}

pub fn tick(handle: &Handle, svc: &ServiceState, lease: &Lease) {
    if lease.job.pipeline().is_some() {
        super::pipeline::tick(handle, svc, lease);
        return;
    }
    if lease.cancelled.load(Ordering::SeqCst) {
        return;
    }
    if !routes::owner_live(svc, &lease.job) {
        let _ = storage::commit(&handle.store, |store| {
            store.stage_stop(&lease.job.id, "creator_gone")
        });
        return;
    }
    if lease.job.pending_capability().is_some() {
        let _ = storage::commit(&handle.store, |store| {
            store.stage_stop(&lease.job.id, "capability_unavailable")
        });
        return;
    }
    if lease.job.input().is_some_and(|input| {
        input.kind != codeterm_blueprint_types::sentinel_input::MediaKind::Text
            || input.convert_to.is_some()
    }) {
        record_input_unavailable(handle, svc, lease);
        return;
    }
    let remaining = {
        let now = handle.store.lock().clock.now_ms();
        lease
            .job
            .expires_at
            .map(|at| Duration::from_millis(at.saturating_sub(now)))
            .unwrap_or(probe::PROBE_RUNTIME)
    };
    let observed = if lease.job.input().is_some() || lease.job.terminal().is_some() {
        let text = handle.inbox.lock().take(&lease.job.id);
        match text {
            Some(text) => Ok(probe::Output {
                bytes: text.as_bytes().to_vec(),
                text,
                success: true,
                exit_code: Some(0),
                ..probe::Output::default()
            }),
            None if lease.job.input().is_some() => {
                super::input::text_output(&lease.job, std::time::Instant::now() + remaining)
                    .map_err(|_| probe::process::ProbeFailure::new("input_text_unavailable"))
            }
            None => return,
        }
    } else if let Some(binding) = &lease.job.observe().cloned() {
        source::observe(svc, &lease.job, binding)
    } else {
        probe::run_with_environment(
            &lease.job.probe(),
            Path::new(&lease.job.cwd),
            remaining,
            &handle.probe_owner,
            &super::probe_env::current_instance(svc.configuration().api),
        )
    };
    let output = resolve_probe_result(
        observed,
        |reason| {
            let decision = if lease.job.observe().is_none() {
                probe_failure::decide(lease.job.last_reason.as_deref(), &reason)
            } else if reason.is_unconfirmed() {
                probe_failure::Decision::StopResources
            } else {
                probe_failure::Decision::Ordinary
            };
            match decision {
                probe_failure::Decision::Skip(stored_reason) => {
                    record_failure(handle, svc, lease, &reason, Some(stored_reason))
                }
                probe_failure::Decision::Stop => {
                    let mut bounded = reason;
                    bounded.primary = "probe_capture_failure_limit";
                    record_unconfirmed_failure(handle, svc, lease, &bounded);
                }
                probe_failure::Decision::StopResources => {
                    record_unconfirmed_failure(handle, svc, lease, &reason)
                }
                probe_failure::Decision::Ordinary => {
                    record_failure(handle, svc, lease, &reason, None)
                }
            }
        },
        std::convert::identity,
    );
    let Some(output) = output else { return };
    if lease.cancelled.load(Ordering::SeqCst) {
        return;
    }
    let remaining = {
        let now = handle.store.lock().clock.now_ms();
        lease
            .job
            .expires_at
            .map(|at| Duration::from_millis(at.saturating_sub(now)))
            .unwrap_or(Duration::from_secs(30))
    };
    advance(
        &handle.store,
        lease,
        &output,
        || classify::classify(svc, &lease.job, &output.text, remaining),
        |job, transition, handler| {
            if !routes::owner_live(svc, job) {
                return Ok(None);
            }
            let staged = handle
                .store
                .lock()
                .stage_claim_handler(job, transition, handler);
            let claimed = staged.settle(&handle.store);
            handle.notify();
            claimed
        },
        |job, transition, handler, verdict| {
            use super::handlers::Status;
            if !routes::owner_live(svc, job) {
                return Err("Blueprint owner or Team is gone".into());
            }
            if handler.target.is_some()
                && crate::service_adapter::graph::target_identity(svc, job, &handler.action)?
                    != handler.target
            {
                return Err("blueprint exact target scope changed".into());
            }
            match &handler.action {
                machine::Route::Sentinel(target) => {
                    let staged = handle.store.lock().stage_activate(job, transition, target);
                    let changed = staged.settle(&handle.store)?;
                    if changed {
                        handle.notify();
                    }
                    Ok(if changed {
                        Status::Completed
                    } else {
                        Status::Noop
                    }
                    .into())
                }
                machine::Route::Command(parameters) => {
                    let remaining = {
                        let store = handle.store.lock();
                        if !store.action_permitted(job, transition) {
                            return Err("command handler source fenced".into());
                        }
                        job.expires_at
                            .map(|at| {
                                Duration::from_millis(at.saturating_sub(store.clock.now_ms()))
                            })
                            .unwrap_or(probe::PROBE_RUNTIME)
                    };
                    Ok(super::command::execute(
                        parameters,
                        Path::new(&job.cwd),
                        remaining,
                        &handle.probe_owner,
                        &lease.cancelled,
                    ))
                }
                route @ machine::Route::RespondPrompt(_) => {
                    if !handle.store.lock().action_permitted(job, transition) {
                        return Err("respond_prompt handler source fenced".into());
                    }
                    super::actions::respond_prompt(svc, job, route, &output)
                }
                machine::Route::Stop => {
                    let staged = {
                        let mut store = handle.store.lock();
                        if !store.action_permitted(job, transition) {
                            return Err("stop handler source fenced".into());
                        }
                        store.stage_stop(&job.id, "stop_handler")
                    };
                    staged.settle(&handle.store)?;
                    handle.notify();
                    Ok(Status::Completed.into())
                }
                route => match routes::run(svc, job, verdict, route, transition, &output.text) {
                    Ok(execution_id) => Ok(super::handlers::Effect {
                        status: Status::Completed,
                        execution_id,
                        command_result: None,
                        reason: None,
                    }),
                    Err(error) if error.starts_with("submit_unconfirmed") => {
                        Ok(Status::Unconfirmed.into())
                    }
                    Err(error) => Err(error),
                },
            }
        },
        |job, outcome, stage| journal(svc, job, outcome, stage),
    );
}

#[cfg(test)]
pub(super) fn fixture_handle_without_app(store: Store) -> Handle {
    Handle {
        store: Arc::new(Mutex::new(store)),
        inbox: Arc::new(Mutex::new(super::terminal::Inbox::default())),
        pending_run_bindings: Arc::new(Mutex::new(BTreeMap::new())),
        wake: Arc::new((std::sync::Mutex::new(0), std::sync::Condvar::new())),
        probe_owner: Arc::new(probe::owner::ProbeResourceOwner::new()),
        app: None,
        _shutdown: None,
    }
}

#[cfg(test)]
pub(super) fn fixture_shutdown_status(handle: &Handle) -> (bool, u64) {
    (
        handle.probe_owner.is_closing(),
        *handle.wake.0.lock().unwrap(),
    )
}
