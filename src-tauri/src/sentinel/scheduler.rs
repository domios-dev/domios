use std::collections::{BTreeMap, BTreeSet};
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
use crate::cron::clock::SharedClock;
use crate::service::{supporter, ServiceState};

const FAILURE_DEADLINE_MS: u64 = 60_000;

#[derive(Clone)]
pub struct Handle {
    pub store: Arc<Mutex<Store>>,
    pending_run_bindings: Arc<Mutex<BTreeMap<String, super::run_binding::PendingRunBinding>>>,
    wake: Arc<(std::sync::Mutex<u64>, std::sync::Condvar)>,
    probe_owner: Arc<probe::owner::ProbeResourceOwner>,
    app: Option<tauri::AppHandle>,
    _shutdown: Option<Arc<crate::crash::shutdown::ShutdownSubscription>>,
}

impl Handle {
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
}

#[cfg(test)]
pub(crate) fn fixture_handle(store: Store, app: tauri::AppHandle) -> Handle {
    Handle {
        store: Arc::new(Mutex::new(store)),
        pending_run_bindings: Arc::new(Mutex::new(BTreeMap::new())),
        wake: Arc::new((std::sync::Mutex::new(0), std::sync::Condvar::new())),
        probe_owner: Arc::new(probe::owner::ProbeResourceOwner::new()),
        app: Some(app),
        _shutdown: None,
    }
}

pub(super) fn emit_changes(store: &Arc<Mutex<Store>>, app: &tauri::AppHandle) {
    let changes = store.lock().take_changes();
    for change in changes {
        crate::api::events::emit_everywhere(app, "automation-graph-changed", change);
    }
}

pub fn start(svc: ServiceState, clock: SharedClock, storage: &crate::storage::Storage) {
    let mut handle = Handle {
        store: Arc::new(Mutex::new(Store::with_storage(storage, clock))),
        pending_run_bindings: Arc::new(Mutex::new(BTreeMap::new())),
        wake: Arc::new((std::sync::Mutex::new(0), std::sync::Condvar::new())),
        probe_owner: Arc::new(probe::owner::ProbeResourceOwner::new()),
        app: Some(svc.app_handle.clone()),
        _shutdown: None,
    };
    let callback = shutdown_callback(&handle);
    match crate::crash::shutdown::subscribe_sentinel_shutdown(callback) {
        Ok(subscription) => handle._shutdown = Some(Arc::new(subscription)),
        Err(error) => {
            log::error!("[sentinel] shutdown registration failed: {error}");
            handle.store.lock().close_admission();
            handle.probe_owner.close_admission();
            return;
        }
    }
    crate::service::sentinel::graph::install_invalidations(&svc, handle.store.clone());
    wake::listen_any(svc.clone(), handle.clone());
    svc.app_handle.manage(handle.clone());
    if let Err(error) = handle.store.lock().ensure_available() {
        log::error!("[sentinel] subsystem unavailable: {error}");
        return;
    }
    let store = handle.store.clone();
    let pending_run_bindings = handle.pending_run_bindings.clone();
    let wake = handle.wake.clone();
    let probe_owner = Arc::downgrade(&handle.probe_owner);
    drop(handle);
    std::thread::spawn(move || {
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
                    log::error!("[sentinel] scheduler unavailable: {error}");
                    unavailable_incident = Some(error.clone());
                }
            } else {
                unavailable_incident = None;
                if let Err(error) = reconciliation {
                    log::error!("[sentinel] unconfirmed commit reconciliation failed: {error}");
                }
                let run_handle = Handle {
                    store: store.clone(),
                    pending_run_bindings: pending_run_bindings.clone(),
                    wake: wake.clone(),
                    probe_owner: owner.clone(),
                    app: Some(svc.app_handle.clone()),
                    _shutdown: None,
                };
                super::run_binding::process_pending(&svc, &run_handle);
                super::run_binding::reconcile_runs(&svc, &run_handle);
                expire_escalations(&store, &svc);
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
                                worker_handle.notify();
                            });
                        }
                    }
                    Err(error) => {
                        if let Some(unavailable) = store.lock().ensure_available().err() {
                            if unavailable_incident.as_ref() != Some(&unavailable) {
                                log::error!("[sentinel] scheduler unavailable: {unavailable}");
                                unavailable_incident = Some(unavailable);
                            }
                        } else {
                            log::error!("[sentinel] schedule persistence failed: {error}");
                        }
                    }
                }
            }
            emit_changes(&store, &svc.app_handle);
            if probe_owner.upgrade().is_none_or(|owner| owner.is_closing()) {
                return;
            }
            let wait_ms = scheduler_wait_ms(&store.lock());
            let guard = match generation.lock() {
                Ok(guard) => guard,
                Err(_) => return,
            };
            if *guard != observed {
                continue;
            }
            let _ =
                wake_condvar.wait_timeout_while(guard, Duration::from_millis(wait_ms), |value| {
                    *value == observed
                });
        }
    });
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
    use crate::sentinel::store::{Create, Owner};
    use serde_json::json;

    #[test]
    fn sentinel_unavailable_wait_uses_failure_deadline_for_overdue_jobs() {
        let temp = tempfile::tempdir().unwrap();
        let clock = crate::cron::clock::MockClock::starting_at(1_000);
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
    latch: &crate::crash::shutdown::ShutdownLatch,
) -> crate::crash::shutdown::ShutdownSubscription {
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
                    log::error!("[sentinel] escalation expiry journal failed: {error}");
                }
            }
            Ok(false) => continue,
            Err(error) => {
                log::error!("[sentinel] escalation expiry persistence failed: {error}");
                return;
            }
        }
    }
}

pub fn record(
    runtime: &supporter::SupporterRuntime,
    job: &Sentinel,
    outcome: &machine::Outcome,
    stage: Stage,
) -> std::io::Result<()> {
    if !outcome.evidence.matches(job) {
        return Err(std::io::Error::other(
            "sentinel observation scope or state hash mismatch",
        ));
    }
    let route_stage = matches!(
        stage,
        Stage::RouteClaim | Stage::RouteCompleted | Stage::RouteFailed
    );
    if route_stage
        && (!matches!(outcome.reason, "transition" | "other" | "low_confidence")
            || outcome.route.is_none()
            || outcome.verdict.is_none())
    {
        return Err(std::io::Error::other(
            "sentinel route stage requires a confident transition",
        ));
    }
    let action = match stage {
        Stage::Decision => "decision",
        Stage::RouteClaim => "route_claim",
        Stage::RouteCompleted => outcome
            .route
            .as_ref()
            .map(routes::action_name)
            .unwrap_or("skip"),
        Stage::EscalationExpired => "escalation_expired",
        Stage::RouteFailed | Stage::Skip => "skip",
    };
    let reason = match stage {
        Stage::RouteFailed => "route_failed",
        Stage::EscalationExpired => "escalation_expired",
        _ => outcome.reason,
    };
    let route_target = match &outcome.route {
        Some(machine::Route::Notify) => Some(job.creator_tab_id.as_str()),
        Some(
            machine::Route::Send(target)
            | machine::Route::Cron(target)
            | machine::Route::Sentinel(target),
        ) => Some(target.as_str()),
        Some(machine::Route::Escalate(machine::EscalateTarget::Creator)) => {
            Some(job.creator_tab_id.as_str())
        }
        Some(machine::Route::Escalate(machine::EscalateTarget::Agent(target))) => {
            Some(target.as_str())
        }
        _ => None,
    };
    runtime.sentinel_event(json!({
        "time": chrono::Utc::now().timestamp(), "kind": "sentinel", "sentinel_id": job.id,
        "tab": job.creator_tab_id, "team_id": job.team_id, "workspace_id": job.workspace_id,
        "state_hash": outcome.evidence.state_hash,
        "evaluation_id": outcome.evidence.evaluation_id,
        "observation_id": outcome.evidence.observation_id,
        "classification_source": outcome.evidence.source,
        "classification_reason": outcome.reason,
        "label": outcome.verdict.as_ref().map(|v| supporter::criteria_hash(v.bucket.as_bytes())),
        "confidence": outcome.verdict.as_ref().map(|v| v.confidence), "action": action, "reason": reason,
        "stage": stage.name(), "route_status": stage.route_status(),
        "route": outcome.route.as_ref().map(routes::action_name), "route_target": route_target,
        "requested_by": outcome.route.as_ref().is_some_and(|route| matches!(route, machine::Route::RespondPrompt(_))).then_some(job.id.as_str()),
        "recipient_receipt": null,
        "asked": supporter::sentinel_questions(&job.id, &job.buckets, outcome.verdict.as_ref().map(|v| v.bucket.as_str()), outcome.reason),
    }))
}

fn record_failure(
    handle: &Handle,
    svc: &ServiceState,
    lease: &Lease,
    failure: &probe::process::ProbeFailure,
) {
    let mut job = lease.job.clone();
    let removed_tab = failure.reason() == "sentinel_observation_tab_removed";
    job.last_reason = Some(if removed_tab {
        "observed_tab_removed".into()
    } else {
        failure.reason()
    });
    let recorded = if removed_tab {
        storage::commit(&handle.store, |store| {
            store.stage_stop(&job.id, "observed_tab_removed")
        })
        .map(|_| true)
    } else {
        storage::commit(&handle.store, |store| store.stage_finish(&job))
    };
    if recorded.unwrap_or(false) {
        let _ = journal(
            svc,
            &job,
            &machine::Outcome {
                evidence: Evidence::new(Scope::from(&lease.job), None),
                reason: failure.primary,
                verdict: None,
                route: None,
                handlers: Vec::new(),
            },
            Stage::Skip,
        );
    }
}

fn record_unconfirmed_failure(
    handle: &Handle,
    svc: &ServiceState,
    lease: &Lease,
    failure: &probe::process::ProbeFailure,
) {
    let result =
        record_unconfirmed_failure_with(&handle.store, &lease.job, failure, |job, out, stage| {
            journal(svc, job, out, stage)
        });
    if let Some(error) = result.persistence_error {
        log::error!(
            "[sentinel] probe stop persistence failed: {error}; resource failure: {}",
            failure.reason()
        );
    }
    if let Some(error) = result.journal_error {
        log::error!(
            "[sentinel] unconfirmed probe skip journal failed: {error}; resource failure: {}",
            failure.reason()
        );
    }
}

pub(super) struct UnconfirmedRecordResult {
    pub persistence_error: Option<String>,
    pub journal_error: Option<String>,
}

pub(super) fn record_unconfirmed_failure_with(
    store: &Arc<Mutex<Store>>,
    job: &Sentinel,
    failure: &probe::process::ProbeFailure,
    mut write_journal: impl FnMut(&Sentinel, &machine::Outcome, Stage) -> std::io::Result<()>,
) -> UnconfirmedRecordResult {
    let reason = failure.reason();
    let (stopped, persistence_error) =
        match storage::commit(store, |current| current.stage_stop(&job.id, &reason)) {
            Ok(stopped) => (Some(stopped), None),
            Err(error) => (store.lock().get(&job.id).cloned(), Some(error)),
        };
    let journal_error = stopped.and_then(|stopped| {
        let outcome = machine::Outcome {
            evidence: Evidence::new(Scope::from(job), None),
            reason: failure.primary,
            verdict: None,
            route: None,
            handlers: Vec::new(),
        };
        write_journal(&stopped, &outcome, Stage::Skip)
            .err()
            .map(|error| error.to_string())
    });
    UnconfirmedRecordResult {
        persistence_error,
        journal_error,
    }
}

pub(super) fn resolve_probe_result<T>(
    result: Result<T, probe::process::ProbeFailure>,
    on_failure: impl FnOnce(probe::process::ProbeFailure),
    on_success: impl FnOnce(T) -> T,
) -> Option<T> {
    match result {
        Ok(output) => Some(on_success(output)),
        Err(failure) => {
            on_failure(failure);
            None
        }
    }
}

pub fn tick(handle: &Handle, svc: &ServiceState, lease: &Lease) {
    if lease.cancelled.load(Ordering::SeqCst) {
        return;
    }
    if !routes::creator_live(svc, &lease.job) {
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
    let remaining = {
        let now = handle.store.lock().clock.now_ms();
        lease
            .job
            .expires_at
            .map(|at| Duration::from_millis(at.saturating_sub(now)))
            .unwrap_or(probe::PROBE_RUNTIME)
    };
    let observed = if let Some(binding) = &lease.job.observe {
        source::observe(svc, &lease.job, binding)
    } else {
        probe::run_with_environment(
            &lease.job.probe,
            Path::new(&lease.job.cwd),
            remaining,
            &handle.probe_owner,
            &probe::environment::Environment::current_instance(),
        )
    };
    let output = resolve_probe_result(
        observed,
        |reason| {
            if reason.is_unconfirmed() {
                record_unconfirmed_failure(handle, svc, lease, &reason);
            } else {
                record_failure(handle, svc, lease, &reason);
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
            if !routes::creator_live(svc, job) {
                return Ok(None);
            }
            let staged = handle
                .store
                .lock()
                .stage_claim_handler(job, transition, handler);
            staged.settle(&handle.store)
        },
        |job, transition, handler, verdict| {
            use super::handlers::Status;
            if !routes::creator_live(svc, job) {
                return Err("sentinel creator or Team is gone".into());
            }
            if handler.target.is_some()
                && crate::service::sentinel::graph::target_identity(svc, job, &handler.action)?
                    != handler.target
            {
                return Err("sentinel exact target scope changed".into());
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

pub(super) fn advance(
    store: &Arc<Mutex<Store>>,
    lease: &Lease,
    output: &probe::Output,
    classify: impl FnOnce() -> super::provenance::Classification,
    mut claim: impl FnMut(
        &Sentinel,
        &str,
        &super::handlers::Handler,
    ) -> Result<Option<super::handlers::ResultRecord>, String>,
    mut execute: impl FnMut(
        &Sentinel,
        &str,
        &super::handlers::Handler,
        &machine::Verdict,
    ) -> Result<super::handlers::Effect, String>,
    mut write_journal: impl FnMut(&Sentinel, &machine::Outcome, Stage) -> std::io::Result<()>,
) {
    use super::handlers::Status;
    if lease.cancelled.load(Ordering::SeqCst) {
        return;
    }
    let current = store.lock().get(&lease.job.id).cloned();
    if !current.is_some_and(|job| {
        job.same_owner(&lease.job)
            && job.revision == lease.job.revision
            && job.machine.last_hash == lease.job.machine.last_hash
            && job.machine.last_bucket == lease.job.machine.last_bucket
            && !job.machine.stopped
            && !job.machine.paused
    }) {
        return;
    }
    let mut job = lease.job.clone();
    let scope = Scope::from(&job);
    let outcome = machine::evaluate_with_other(
        &mut job.machine,
        &output.bytes,
        &job.buckets,
        &job.routes,
        &job.on_other,
        job.until.as_deref(),
        scope,
        classify,
    );
    job.last_reason = Some(outcome.reason.into());
    let transition_bucket = match outcome.reason {
        "transition" => outcome
            .verdict
            .as_ref()
            .map(|verdict| verdict.bucket.as_str()),
        "other" | "low_confidence" if !outcome.handlers.is_empty() => Some("other"),
        _ => None,
    };
    let committed = if let Some(bucket) = transition_bucket {
        storage::commit(store, |current| {
            current.stage_accept_transition(&job, bucket, &outcome.evidence.evaluation_id)
        })
    } else {
        storage::commit(store, |current| current.stage_finish(&job))
    };
    if !matches!(committed, Ok(true)) {
        return;
    }
    if write_journal(&job, &outcome, Stage::Decision).is_err() {
        let _ = storage::commit(store, |current| {
            current.stage_skip_remaining(
                &job.id,
                &outcome.evidence.evaluation_id,
                &outcome.handlers,
            )
        });
        return;
    }
    for (index, handler) in outcome.handlers.iter().enumerate() {
        let transition = &outcome.evidence.evaluation_id;
        let permitted = !lease.cancelled.load(Ordering::SeqCst)
            && store.lock().action_permitted(&job, transition);
        if !permitted {
            let _ = storage::commit(store, |current| {
                current.stage_skip_remaining(&job.id, transition, &outcome.handlers[index..])
            });
            return;
        }
        let accepted = match claim(&job, transition, handler) {
            Ok(Some(claim)) => claim,
            Err(error) if error == super::handlers::ESCALATION_RATE_LIMITED => {
                let mut limited = outcome.clone();
                limited.reason = "escalation_rate_limited";
                limited.route = Some(handler.action.clone());
                let _ = write_journal(&job, &limited, Stage::Skip);
                if storage::commit(store, |current| {
                    current.stage_skip_remaining(&job.id, transition, std::slice::from_ref(handler))
                })
                .is_err()
                {
                    return;
                }
                continue;
            }
            _ => {
                let _ = storage::commit(store, |current| {
                    current.stage_skip_remaining(&job.id, transition, &outcome.handlers[index..])
                });
                return;
            }
        };
        let mut handler_outcome = outcome.clone();
        handler_outcome.route = Some(handler.action.clone());
        if write_journal(&job, &handler_outcome, Stage::RouteClaim).is_err() {
            let _ = storage::commit(store, |current| {
                current.stage_skip_remaining(&job.id, transition, &outcome.handlers[index + 1..])
            });
            return;
        }
        if lease.cancelled.load(Ordering::SeqCst)
            || !store.lock().action_permitted(&job, transition)
        {
            let _ = storage::commit(store, |current| {
                current.stage_skip_remaining(&job.id, transition, &outcome.handlers[index + 1..])
            });
            return;
        }
        let Some(verdict) = outcome.verdict.as_ref() else {
            return;
        };
        let result = execute(&job, transition, handler, verdict);
        let stage = if matches!(
            &result,
            Ok(super::handlers::Effect {
                status: Status::Completed | Status::Noop,
                ..
            })
        ) {
            Stage::RouteCompleted
        } else {
            Stage::RouteFailed
        };
        let effect = result.unwrap_or_else(|_| Status::Failed.into());
        let unsafe_continuation = effect
            .command_result
            .as_ref()
            .is_some_and(|result| !result.resources_confirmed);
        if let Err(error) = storage::commit(store, |current| {
            current.stage_finish_handler_result(&job.id, transition, &accepted, effect)
        }) {
            log::error!("[sentinel] handler result persistence failed: {error}");
            return;
        }
        let _ = write_journal(&job, &handler_outcome, stage);
        if unsafe_continuation {
            let _ = storage::commit(store, |current| {
                current.stage_stop(&job.id, "command_resources_unconfirmed")
            });
            let _ = storage::commit(store, |current| {
                current.stage_skip_remaining(&job.id, transition, &outcome.handlers[index + 1..])
            });
            return;
        }
    }
}

#[cfg(test)]
pub(super) fn fixture_handle_without_app(store: Store) -> Handle {
    Handle {
        store: Arc::new(Mutex::new(store)),
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
