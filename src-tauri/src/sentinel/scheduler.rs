use std::path::Path;
use std::sync::{atomic::Ordering, Arc};
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::json;
use tauri::Manager;

use super::{
    classify, machine, probe,
    provenance::{Evidence, Scope, Stage},
    routes,
    store::{Lease, Sentinel, Store},
};
use crate::cron::clock::SharedClock;
use crate::service::{supporter, ServiceState};

#[derive(Clone)]
pub struct Handle {
    pub store: Arc<Mutex<Store>>,
    wake: Arc<(std::sync::Mutex<u64>, std::sync::Condvar)>,
    probe_owner: Arc<probe::owner::ProbeResourceOwner>,
    app: Option<tauri::AppHandle>,
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

    pub fn remove_team(&self, team: &str) -> Result<(), String> {
        let result = self.store.lock().remove_team(team);
        self.notify();
        result
    }
}

#[cfg(test)]
pub(crate) fn fixture_handle(store: Store, app: tauri::AppHandle) -> Handle {
    Handle {
        store: Arc::new(Mutex::new(store)),
        wake: Arc::new((std::sync::Mutex::new(0), std::sync::Condvar::new())),
        probe_owner: Arc::new(probe::owner::ProbeResourceOwner::new()),
        app: Some(app),
    }
}

pub(super) fn emit_changes(store: &Arc<Mutex<Store>>, app: &tauri::AppHandle) {
    let changes = store.lock().take_changes();
    for change in changes {
        crate::api::events::emit_everywhere(app, "automation-graph-changed", change);
    }
}

pub fn start(svc: ServiceState, clock: SharedClock) {
    let handle = Handle {
        store: Arc::new(Mutex::new(Store::new(
            codeterm_paths::data_dir().join("sentinels.json"),
            clock,
        ))),
        wake: Arc::new((std::sync::Mutex::new(0), std::sync::Condvar::new())),
        probe_owner: Arc::new(probe::owner::ProbeResourceOwner::new()),
        app: Some(svc.app_handle.clone()),
    };
    crate::service::sentinel::graph::install_invalidations(&svc, handle.store.clone());
    svc.app_handle.manage(handle.clone());
    if let Err(error) = handle.store.lock().ensure_available() {
        log::error!("[sentinel] subsystem unavailable: {error}");
        return;
    }
    let store = handle.store.clone();
    let wake = handle.wake.clone();
    let probe_owner = Arc::downgrade(&handle.probe_owner);
    drop(handle);
    std::thread::spawn(move || {
        let (generation, wake_condvar) = &*wake;
        loop {
            let observed = match generation.lock() {
                Ok(guard) => *guard,
                Err(_) => return,
            };
            let due = store.lock().due();
            match due {
                Ok(leases) => {
                    for lease in leases {
                        let Some(owner) = probe_owner.upgrade() else {
                            return;
                        };
                        let worker_handle = Handle {
                            store: store.clone(),
                            wake: wake.clone(),
                            probe_owner: owner,
                            app: Some(svc.app_handle.clone()),
                        };
                        let svc = svc.clone();
                        std::thread::spawn(move || {
                            tick(&worker_handle, &svc, &lease);
                            worker_handle.store.lock().release(&lease.job.id);
                            worker_handle.notify();
                        });
                    }
                }
                Err(error) => log::error!("[sentinel] schedule persistence failed: {error}"),
            }
            emit_changes(&store, &svc.app_handle);
            let wait_ms = store.lock().next_deadline().max(1);
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
        && (outcome.reason != "transition" || outcome.route.is_none() || outcome.verdict.is_none())
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
        Stage::RouteFailed | Stage::Skip => "skip",
    };
    let reason = if stage == Stage::RouteFailed {
        "route_failed"
    } else {
        outcome.reason
    };
    let route_target = match &outcome.route {
        Some(machine::Route::Notify) => Some(job.creator_tab_id.as_str()),
        Some(
            machine::Route::Send(target)
            | machine::Route::Cron(target)
            | machine::Route::Sentinel(target),
        ) => Some(target.as_str()),
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
    job.last_reason = Some(failure.reason());
    if handle.store.lock().finish(&job).unwrap_or(false) {
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
    let (stopped, persistence_error) = {
        let mut store = store.lock();
        match store.stop(&job.id, &reason) {
            Ok(stopped) => (Some(stopped), None),
            Err(error) => (store.get(&job.id).cloned(), Some(error)),
        }
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
        let _ = handle.store.lock().stop(&lease.job.id, "creator_gone");
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
    let output = resolve_probe_result(
        probe::run(
            &lease.job.probe,
            Path::new(&lease.job.cwd),
            remaining,
            &handle.probe_owner,
        ),
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
            let mux = svc.mux.read();
            let live = mux
                .get_tab(&job.creator_tab_id)
                .is_some_and(|tab| tab.team_id == job.team_id)
                && mux.find_workspace_for_tab(&job.creator_tab_id).as_deref()
                    == Some(&job.workspace_id)
                && job
                    .team_id
                    .as_ref()
                    .is_none_or(|id| mux.get_team(id).is_some());
            if !live {
                return Ok(None);
            }
            handle.store.lock().claim_handler(job, transition, handler)
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
                    let mux = svc.mux.read();
                    let live = mux
                        .get_tab(&job.creator_tab_id)
                        .is_some_and(|tab| tab.team_id == job.team_id)
                        && mux.find_workspace_for_tab(&job.creator_tab_id).as_deref()
                            == Some(&job.workspace_id)
                        && job
                            .team_id
                            .as_ref()
                            .is_none_or(|id| mux.get_team(id).is_some());
                    if !live {
                        return Err("sentinel creator or Team is gone".into());
                    }
                    let changed = handle.store.lock().activate(job, transition, target)?;
                    drop(mux);
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
                machine::Route::Stop => {
                    {
                        let mut store = handle.store.lock();
                        if !store.action_permitted(job, transition) {
                            return Err("stop handler source fenced".into());
                        }
                        store.stop(&job.id, "stop_handler")?;
                    }
                    handle.notify();
                    Ok(Status::Completed.into())
                }
                route => match routes::run(svc, job, verdict, route, &output.text) {
                    Ok(execution_id) => Ok(super::handlers::Effect {
                        status: Status::Completed,
                        execution_id,
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
    let outcome = machine::evaluate(
        &mut job.machine,
        &output.bytes,
        &job.buckets,
        &job.routes,
        job.until.as_deref(),
        scope,
        classify,
    );
    job.last_reason = Some(outcome.reason.into());
    let committed = if outcome.reason == "transition" {
        store.lock().accept_transition(
            &job,
            &outcome
                .verdict
                .as_ref()
                .map(|v| v.bucket.clone())
                .unwrap_or_default(),
            &outcome.evidence.evaluation_id,
        )
    } else {
        store.lock().finish(&job)
    };
    if !matches!(committed, Ok(true)) {
        return;
    }
    if write_journal(&job, &outcome, Stage::Decision).is_err() {
        let _ = store.lock().skip_remaining(
            &job.id,
            &outcome.evidence.evaluation_id,
            &outcome.handlers,
        );
        return;
    }
    for (index, handler) in outcome.handlers.iter().enumerate() {
        let transition = &outcome.evidence.evaluation_id;
        let permitted = !lease.cancelled.load(Ordering::SeqCst)
            && store.lock().action_permitted(&job, transition);
        if !permitted {
            let _ = store
                .lock()
                .skip_remaining(&job.id, transition, &outcome.handlers[index..]);
            return;
        }
        let accepted = match claim(&job, transition, handler) {
            Ok(Some(claim)) => claim,
            _ => {
                let _ =
                    store
                        .lock()
                        .skip_remaining(&job.id, transition, &outcome.handlers[index..]);
                return;
            }
        };
        let mut handler_outcome = outcome.clone();
        handler_outcome.route = Some(handler.action.clone());
        if write_journal(&job, &handler_outcome, Stage::RouteClaim).is_err() {
            let _ =
                store
                    .lock()
                    .skip_remaining(&job.id, transition, &outcome.handlers[index + 1..]);
            return;
        }
        if lease.cancelled.load(Ordering::SeqCst)
            || !store.lock().action_permitted(&job, transition)
        {
            let _ =
                store
                    .lock()
                    .skip_remaining(&job.id, transition, &outcome.handlers[index + 1..]);
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
        if let Err(error) = store
            .lock()
            .finish_handler_result(&job.id, transition, &accepted, effect)
        {
            log::error!("[sentinel] handler result persistence failed: {error}");
            return;
        }
        let _ = write_journal(&job, &handler_outcome, stage);
    }
}
