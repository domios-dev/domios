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
}

impl Handle {
    pub fn notify(&self) {
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

pub fn start(svc: ServiceState, clock: SharedClock) {
    let handle = Handle {
        store: Arc::new(Mutex::new(Store::new(
            codeterm_paths::data_dir().join("sentinels.json"),
            clock,
        ))),
        wake: Arc::new((std::sync::Mutex::new(0), std::sync::Condvar::new())),
    };
    svc.app_handle.manage(handle.clone());
    std::thread::spawn(move || {
        let (generation, wake) = &*handle.wake;
        loop {
            let observed = match generation.lock() {
                Ok(guard) => *guard,
                Err(_) => return,
            };
            let due = handle.store.lock().due();
            match due {
                Ok(leases) => {
                    for lease in leases {
                        let worker_handle = handle.clone();
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
            let wait_ms = handle.store.lock().next_deadline().max(1);
            let guard = match generation.lock() {
                Ok(guard) => guard,
                Err(_) => return,
            };
            if *guard != observed {
                continue;
            }
            let _ = wake.wait_timeout_while(guard, Duration::from_millis(wait_ms), |value| {
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
        Some(machine::Route::Send(target) | machine::Route::Cron(target)) => Some(target.as_str()),
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

fn record_failure(handle: &Handle, svc: &ServiceState, lease: &Lease, reason: &'static str) {
    let mut job = lease.job.clone();
    job.last_reason = Some(reason.into());
    if handle.store.lock().finish(&job).unwrap_or(false) {
        let _ = journal(
            svc,
            &job,
            &machine::Outcome {
                evidence: Evidence::new(Scope::from(&lease.job), None),
                reason,
                verdict: None,
                route: None,
            },
            Stage::Skip,
        );
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
    let output = match probe::run(&lease.job.probe, Path::new(&lease.job.cwd), remaining) {
        Ok(output) => output,
        Err(reason) => {
            record_failure(handle, svc, lease, reason);
            return;
        }
    };
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
    let mut job = lease.job.clone();
    let scope = Scope::from(&job);
    let outcome = machine::evaluate(
        &mut job.machine,
        &output.bytes,
        &job.buckets,
        &job.routes,
        job.until.as_deref(),
        scope,
        || classify::classify(svc, &lease.job, &output.text, remaining),
    );
    job.last_reason = Some(outcome.reason.into());
    let committed = handle.store.lock().finish(&job);
    match committed {
        Ok(true) => {}
        Ok(false) => return,
        Err(error) => {
            log::error!("[sentinel] transition persistence failed: {error}");
            return;
        }
    }
    if journal(svc, &job, &outcome, Stage::Decision).is_err() {
        return;
    }
    if let (Some(route), Some(verdict)) = (&outcome.route, &outcome.verdict) {
        if lease.cancelled.load(Ordering::SeqCst) {
            return;
        }
        let expired = job
            .expires_at
            .is_some_and(|at| handle.store.lock().clock.now_ms() >= at);
        if expired {
            return;
        }
        if journal(svc, &job, &outcome, Stage::RouteClaim).is_err() {
            return;
        }
        if lease.cancelled.load(Ordering::SeqCst)
            || job
                .expires_at
                .is_some_and(|at| handle.store.lock().clock.now_ms() >= at)
        {
            return;
        }
        match routes::run(svc, &job, verdict, route, &output.text) {
            Ok(()) => {
                let _ = journal(svc, &job, &outcome, Stage::RouteCompleted);
            }
            Err(error) => {
                log::warn!("[sentinel] route failed id={}: {error}", job.id);
                let _ = journal(svc, &job, &outcome, Stage::RouteFailed);
            }
        }
    }
}
