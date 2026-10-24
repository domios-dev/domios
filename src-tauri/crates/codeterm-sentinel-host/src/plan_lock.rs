use crate::{scheduler::Handle, storage, store::Sentinel};
use codeterm_sentinel::admission::HeldReason;
use codeterm_service_core::multiplexer::state::MuxLocks;
use codeterm_service_core::service::{billing::LimitKey, ServiceContext};

pub(crate) fn locked(locks: &MuxLocks, job: &Sentinel) -> bool {
    locks.is_locked(LimitKey::Workspaces, &job.workspace_id)
        || job
            .team_id
            .as_deref()
            .is_some_and(|id| locks.is_locked(LimitKey::Teams, id))
        || (job.document.as_ref().and_then(|doc| doc["owner"].as_str()) != Some("team")
            && locks.is_locked(LimitKey::AgentTabs, &job.creator_tab_id))
}

pub(crate) fn refuse(service: &ServiceContext, job: &Sentinel) -> Result<(), String> {
    if locked(&service.locks(), job) {
        Err("plan_limit_locked: the Blueprint's owning item is locked by your plan".into())
    } else {
        Ok(())
    }
}

pub(crate) fn project(service: &ServiceContext, jobs: &mut [Sentinel]) {
    let locks = service.locks();
    for job in jobs {
        let is_locked = locked(&locks, job);
        let watch = job.pipeline().is_none() && job.input().is_none() && job.terminal().is_none();
        job.admission.reconcile(is_locked, watch);
    }
}

pub(crate) fn reconcile(service: &ServiceContext, handle: &Handle) -> Result<(), String> {
    let locks = service.locks();
    let ids = handle
        .store
        .lock()
        .list()
        .iter()
        .map(|job| job.id.clone())
        .collect::<Vec<_>>();
    for id in ids {
        let changed = storage::commit(&handle.store, |store| {
            let Some(mut job) = store.get(&id).cloned() else {
                return storage::Staged::ready(Ok(false));
            };
            let is_locked = locked(&locks, &job);
            let watch =
                job.pipeline().is_none() && job.input().is_none() && job.terminal().is_none();
            if !job.admission.reconcile(is_locked, watch) {
                return storage::Staged::ready(Ok(false));
            }
            store.wake_pending.remove(&id);
            store.stage_admission(&id, job.admission)
        })?;
        if changed {
            handle.notify();
        }
    }
    Ok(())
}

pub(crate) fn admitted(service: &ServiceContext, handle: &Handle, job: &Sentinel) -> bool {
    if refuse(service, job).is_err() {
        if let Err(error) = reconcile(service, handle) {
            log::error!("[blueprint] plan hold persistence failed: {error}");
        }
        return false;
    }
    handle
        .store
        .lock()
        .get(&job.id)
        .is_some_and(|current| current.admission.permits_steps())
}

pub(crate) fn held_reason(job: &Sentinel) -> Option<&'static str> {
    job.admission.held_reason.map(HeldReason::as_str)
}

#[cfg(test)]
mod tests;
