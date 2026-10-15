use super::source::{self, RunBinding, TabBinding};
use super::store::Sentinel;
use codeterm_service_core::cron::store::{CronJob, CronRunInfo, CronRunStatus};
use codeterm_service_core::service::ServiceContext;
use serde_json::{json, Value};
use tauri::Manager;

const MAX_CRON_EVENT_BYTES: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PendingRunBinding {
    pub(super) cron_id: String,
    pub(super) run_id: String,
    pub(super) tab_id: String,
    pub(super) started_at: u64,
    pub(super) team_id: Option<String>,
    pub(super) workspace_id: String,
}

pub(super) struct CronFiredEvent {
    pub(super) cron_id: String,
    pub(super) run_id: String,
    pub(super) tab_id: String,
}

pub(super) fn queue_fired_event(
    svc: &ServiceContext,
    handle: &super::scheduler::Handle,
    payload: &str,
) -> bool {
    let Some(event) = cron_fired_event(payload) else {
        return false;
    };
    let Some(binding) = verified_event(svc, event) else {
        return false;
    };
    let ids = {
        let store = handle.store.lock();
        store
            .list()
            .iter()
            .filter(|job| {
                !job.machine.stopped
                    && job.observe().cloned() == Some(TabBinding::CronRun(binding.cron_id.clone()))
                    && job.team_id == binding.team_id
                    && job.workspace_id == binding.workspace_id
            })
            .map(|job| job.id.clone())
            .collect::<Vec<_>>()
    };
    let mut queued = false;
    for id in ids {
        queued |= handle.queue_run_binding(id, binding.clone());
    }
    queued
}

pub(super) fn cron_fired_event(payload: &str) -> Option<CronFiredEvent> {
    if payload.len() > MAX_CRON_EVENT_BYTES {
        return None;
    }
    let value: Value = serde_json::from_str(payload).ok()?;
    Some(CronFiredEvent {
        cron_id: value.get("cron_id")?.as_str()?.to_string(),
        run_id: value.get("run_id")?.as_str()?.to_string(),
        tab_id: value.get("tab_id")?.as_str()?.to_string(),
    })
}

fn cron_identity(svc: &ServiceContext, cron_id: &str) -> Option<CronJob> {
    let state = svc
        .app_handle
        .try_state::<codeterm_service_core::CronState>()?;
    let job = state.lock().get(cron_id).cloned();
    job
}

fn verified_event(svc: &ServiceContext, event: CronFiredEvent) -> Option<PendingRunBinding> {
    if !source::bounded_id(&event.cron_id)
        || !source::bounded_id(&event.run_id)
        || !source::bounded_id(&event.tab_id)
    {
        return None;
    }
    let state = svc
        .app_handle
        .try_state::<codeterm_service_core::CronState>()?;
    let cron = state.lock();
    let job = cron.get(&event.cron_id)?;
    let run = cron
        .get_history(&event.cron_id)
        .into_iter()
        .find(|run| run.run_id == event.run_id)?;
    active_event_run(job, run, &event.tab_id)
}

pub(super) fn active_event_run(
    job: &CronJob,
    run: &CronRunInfo,
    event_tab_id: &str,
) -> Option<PendingRunBinding> {
    let tab_id = run.tab_id.as_deref()?;
    if tab_id != event_tab_id
        || run.finished_at.is_some()
        || !matches!(
            &run.status,
            &CronRunStatus::Running | &CronRunStatus::AwaitingOwnerPrompt
        )
        || !job
            .last_run
            .as_ref()
            .is_some_and(|last| last.run_id == run.run_id)
    {
        return None;
    }
    Some(PendingRunBinding {
        cron_id: job.id.clone(),
        run_id: run.run_id.clone(),
        tab_id: tab_id.to_string(),
        started_at: run.started_at,
        team_id: job.team_id.clone(),
        workspace_id: job.workspace_id.clone(),
    })
}

pub(super) fn process_pending(svc: &ServiceContext, handle: &super::scheduler::Handle) {
    for (id, pending) in handle.take_run_bindings() {
        let Some(binding) = verify_pending(svc, &pending) else {
            continue;
        };
        let Some(job) = handle.store.lock().get(&id).cloned() else {
            continue;
        };
        let Some(cron) = cron_identity(svc, &binding.cron_id) else {
            continue;
        };
        if job.observe().cloned() != Some(TabBinding::CronRun(binding.cron_id.clone()))
            || job.team_id != cron.team_id
            || job.workspace_id != cron.workspace_id
            || source::validate_pending_cron_run_scope(
                &svc.mux.read(),
                &job,
                &binding.cron_id,
                &binding.tab_id,
            )
            .is_err()
        {
            continue;
        }
        match super::storage::commit(&handle.store, |store| {
            store.stage_bind_run(&id, &binding.cron_id, &binding.run_id, &binding.tab_id)
        }) {
            Ok(outcome) if outcome.changed => {
                if let Some(previous) = outcome.replaced {
                    journal_replacement(svc, &job, &binding, &previous);
                }
            }
            Ok(_) => {}
            Err(error) => log::error!("[blueprint] cron run binding failed: {error}"),
        }
    }
}

fn verify_pending(svc: &ServiceContext, pending: &PendingRunBinding) -> Option<PendingRunBinding> {
    let state = svc
        .app_handle
        .try_state::<codeterm_service_core::CronState>()?;
    let cron = state.lock();
    let job = cron.get(&pending.cron_id)?;
    let run = cron
        .get_history(&pending.cron_id)
        .into_iter()
        .find(|run| run.run_id == pending.run_id)?;
    active_event_run(job, run, &pending.tab_id)
}

fn journal_replacement(
    svc: &ServiceContext,
    job: &Sentinel,
    binding: &PendingRunBinding,
    previous: &RunBinding,
) {
    if !job.model_classification {
        return;
    }
    let event = replacement_event(job, binding, previous);
    if let Err(error) = codeterm_service_core::service::supporter::runtime_with_notifications(svc)
        .sentinel_event(event)
    {
        log::warn!("[blueprint] run replacement journal failed: {error}");
    }
}

pub(super) fn replacement_event(
    job: &Sentinel,
    binding: &PendingRunBinding,
    previous: &RunBinding,
) -> serde_json::Value {
    json!({
        "time": chrono::Utc::now().timestamp(),
        "kind": "blueprint",
        "tab": job.creator_tab_id,
        "workspace_id": job.workspace_id,
        "team_id": job.team_id,
        "action": "run_replaced",
        "reason": "run_replaced",
        "label": "other",
        "blueprint_id": job.id,
        "cron_id": binding.cron_id,
        "old_run_id": previous.run_id,
        "new_run_id": binding.run_id,
        "run_tab_id": binding.tab_id
    })
}

pub(super) fn reconcile_runs(svc: &ServiceContext, handle: &super::scheduler::Handle) {
    let bound: Vec<_> = handle
        .store
        .lock()
        .list()
        .iter()
        .filter_map(|job| {
            let run = job.run.as_ref()?;
            let Some(TabBinding::CronRun(cron_id)) = job.observe() else {
                return None;
            };
            Some((job.clone(), cron_id.clone(), run.clone()))
        })
        .collect();
    if bound.is_empty() {
        return;
    }
    let Some(state) = svc
        .app_handle
        .try_state::<codeterm_service_core::CronState>()
    else {
        unbind_missing_cron(svc, handle, bound);
        return;
    };
    let snapshots: Vec<_> = {
        let cron = state.lock();
        bound
            .into_iter()
            .map(|(job, cron_id, run)| {
                let cron_job = cron.get(&cron_id).cloned();
                let run_info = cron
                    .get_history(&cron_id)
                    .into_iter()
                    .find(|entry| entry.run_id == run.run_id)
                    .cloned();
                (job, cron_id, run, cron_job, run_info)
            })
            .collect()
    };
    let stale = {
        let mux = svc.mux.read();
        let mut stale = Vec::new();
        for (job, cron_id, run, cron_job, run_info) in snapshots {
            let tab_missing = mux.get_tab(&run.tab_id).is_none();
            let same_scope = cron_job.as_ref().is_some_and(|cron| {
                cron.team_id == job.team_id && cron.workspace_id == job.workspace_id
            });
            let tab_in_scope = source::validate_tab_scope(&mux, &job, &run.tab_id).is_ok();
            let run_latest = cron_job.as_ref().is_some_and(|cron| {
                cron.last_run.as_ref().is_some_and(|latest| {
                    latest.run_id == run.run_id
                        && latest.tab_id.as_deref() == Some(run.tab_id.as_str())
                })
            });
            let run_active = run_latest
                && run_info.as_ref().is_some_and(|info| {
                    info.finished_at.is_none()
                        && info.tab_id.as_deref() == Some(run.tab_id.as_str())
                        && matches!(
                            &info.status,
                            &CronRunStatus::Running | &CronRunStatus::AwaitingOwnerPrompt
                        )
                });
            if !same_scope || !tab_in_scope || !run_active {
                let reason = if tab_missing {
                    "tab_gone"
                } else {
                    "cron_run_finished"
                };
                stale.push((job, cron_id, run, reason));
            }
        }
        stale
    };
    for (job, cron_id, run, reason) in stale {
        match super::storage::commit(&handle.store, |store| {
            store.stage_unbind_run(&job.id, &run, reason)
        }) {
            Ok(outcome)
                if outcome.changed && matches!(reason, "tab_gone" | "cron_run_finished") =>
            {
                super::routes::notify_run_unbound(
                    svc,
                    &super::store::StopNotice::new(&job, reason),
                );
            }
            Ok(_) => {}
            Err(error) => {
                log::error!("[blueprint] cron run unbinding failed: {error}; cron={cron_id}");
            }
        }
    }
}

fn unbind_missing_cron(
    svc: &ServiceContext,
    handle: &super::scheduler::Handle,
    bound: Vec<(Sentinel, String, RunBinding)>,
) {
    for (job, _, run) in bound {
        match super::storage::commit(&handle.store, |store| {
            store.stage_unbind_run(&job.id, &run, "cron_run_finished")
        }) {
            Ok(outcome) if outcome.changed => super::routes::notify_run_unbound(
                svc,
                &super::store::StopNotice::new(&job, "cron_run_finished"),
            ),
            Ok(_) => {}
            Err(error) => log::error!("[blueprint] cron run unbinding failed: {error}"),
        }
    }
}
