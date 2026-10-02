use std::path::Path;
use std::sync::{atomic::Ordering, Arc};
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::json;
use tauri::Manager;

use super::{
    classify, machine, probe, routes,
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
    action: &str,
) -> std::io::Result<()> {
    record(
        &supporter::runtime_with_notifications(svc),
        job,
        outcome,
        action,
    )
}

pub fn record(
    runtime: &supporter::SupporterRuntime,
    job: &Sentinel,
    outcome: &machine::Outcome,
    action: &str,
) -> std::io::Result<()> {
    runtime.sentinel_event(json!({
        "time": chrono::Utc::now().timestamp(), "kind": "sentinel", "sentinel_id": job.id,
        "tab": job.creator_tab_id, "team_id": job.team_id, "state_hash": job.machine.last_hash,
        "label": outcome.verdict.as_ref().map(|v| supporter::criteria_hash(v.bucket.as_bytes())),
        "confidence": outcome.verdict.as_ref().map(|v| v.confidence), "action": action, "reason": outcome.reason,
        "route": outcome.route.as_ref().map(routes::action_name),
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
                reason,
                verdict: None,
                route: None,
            },
            "skip",
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
    let outcome = machine::evaluate(
        &mut job.machine,
        &output.bytes,
        &job.buckets,
        &job.routes,
        job.until.as_deref(),
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
    if outcome.reason != "unchanged" && journal(svc, &job, &outcome, "decision").is_err() {
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
        if journal(svc, &job, &outcome, "route_claim").is_err() {
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
                let _ = journal(svc, &job, &outcome, routes::action_name(route));
            }
            Err(error) => {
                log::warn!("[sentinel] route failed id={}: {error}", job.id);
                let failed = machine::Outcome {
                    reason: "route_failed",
                    verdict: outcome.verdict.clone(),
                    route: None,
                };
                let _ = journal(svc, &job, &failed, "skip");
            }
        }
    }
}
