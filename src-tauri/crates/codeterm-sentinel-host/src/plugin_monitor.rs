use crate::{pipeline::plugin, scheduler::Handle, storage};
use codeterm_plugin::{
    blueprint_step::*, graph_nodes::GraphNodeRegistry, view_host::ViewOwnerScope,
};
use codeterm_service_core::service::{ServiceContext, ServiceState};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

struct CheckObserver {
    deadline: Instant,
    remaining: AtomicUsize,
    service: std::sync::Weak<ServiceContext>,
    scope: PluginJobScope,
}
impl codeterm_decision::physical::RequestObserver for CheckObserver {
    fn before_request(&self) -> Result<(), codeterm_decision::DecisionError> {
        if !self.effects_allowed() {
            return Err(codeterm_decision::DecisionError::Deadline);
        }
        self.remaining
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                remaining.checked_sub(1)
            })
            .map(|_| ())
            .map_err(|_| codeterm_decision::DecisionError::Budget)
    }
    fn effects_allowed(&self) -> bool {
        Instant::now() < self.deadline
            && self
                .service
                .upgrade()
                .is_some_and(|service| live(&service, &self.scope))
    }
    fn deadline(&self) -> Option<Instant> {
        Some(self.deadline)
    }
}

fn live(service: &ServiceContext, scope: &PluginJobScope) -> bool {
    let mux = service.mux.read();
    mux.get_tab(&scope.owner)
        .is_some_and(|owner| owner.team_id == scope.team)
        && mux.find_workspace_for_tab(&scope.owner).as_deref() == Some(scope.workspace.as_str())
}

fn prepare_monitors(service: &ServiceContext, handle: &Handle) -> Result<(), String> {
    let ledger = plugin::ledger(service)?;
    let now = handle.store.lock().clock.now_ms();
    let jobs = handle.store.lock().list().to_vec();
    for job in &jobs {
        let Some(held) = &job.admission.plugin_hold else {
            continue;
        };
        let Some(mut record) = ledger.get(&held.operation)? else {
            continue;
        };
        if record.request.context.scope.workspace != job.workspace_id
            || record.request.context.scope.team != job.team_id
            || record.request.context.owner != job.creator_tab_id
            || record.request.context.scope.run != held.run
            || record.request.context.node != held.node
        {
            continue;
        }
        if record.state.terminal() {
            if job.machine.stopped {
                if let Some(monitor) = ledger.job_monitor(&held.operation)? {
                    sync_hold(
                        handle,
                        &monitor.registration,
                        record.state,
                        "plugin_step_stopped",
                    )?;
                }
                continue;
            }
            if ledger
                .job_monitor(&held.operation)?
                .is_none_or(|monitor| monitor.stopped.is_none())
            {
                resume(handle, &job.id, &held.operation)?;
            }
            continue;
        }
        let Some(job_id) = record.job.clone() else {
            continue;
        };
        let stopping =
            job.machine.stopped || job.expires_at.is_some_and(|deadline| deadline <= now);
        if stopping {
            record = ledger.request_cancel(
                &held.operation,
                &record.request.context.scope,
                &record.request.context.owner,
            )?;
        }
        if ledger.job_monitor(&held.operation)?.is_none() {
            ledger.monitor_existing_job(
                PluginJobRegistration {
                    scope: PluginJobScope {
                        plugin: record.plugin.clone(),
                        workspace: job.workspace_id.clone(),
                        team: job.team_id.clone(),
                        owner: job.creator_tab_id.clone(),
                    },
                    origin: PluginJobOrigin::Blueprint {
                        blueprint: job.id.clone(),
                        run: held.run.clone(),
                        node: held.node.clone(),
                        attempt: record.request.context.attempt.clone(),
                    },
                    operation: held.operation.clone(),
                    job: job_id,
                    next_check_ms: now.saturating_add(1000),
                    deadline_ms: now.saturating_add(7 * 24 * 60 * 60 * 1000),
                },
                now,
            )?;
        }
        if stopping {
            let monitor = ledger
                .job_monitor(&held.operation)?
                .ok_or("plugin monitor unavailable")?;
            if let Some(reason) = monitor.stopped {
                sync_hold(
                    handle,
                    &monitor.registration,
                    OperationState::Unknown,
                    stop_reason(reason),
                )?;
            } else {
                ledger.request_job_cancel(&held.operation, &monitor.registration.scope)?;
                sync_hold(
                    handle,
                    &monitor.registration,
                    OperationState::Unknown,
                    "plugin_step_canceling",
                )?;
            }
        }
    }
    for monitor in ledger.active_job_monitors()? {
        let registration = &monitor.registration;
        let stopped = if now >= registration.deadline_ms {
            Some(MonitorStop::Deadline)
        } else if monitor.checks >= 4096 && !ledger.monitor_in_flight(&registration.operation) {
            Some(MonitorStop::CheckLimit)
        } else if !live(service, &registration.scope) {
            Some(MonitorStop::ScopeRemoved)
        } else if let PluginJobOrigin::Blueprint { blueprint, .. } = &registration.origin {
            match jobs.iter().find(|job| &job.id == blueprint) {
                None => Some(MonitorStop::ScopeRemoved),
                Some(job)
                    if job.machine.stopped
                        || job.expires_at.is_some_and(|deadline| deadline <= now) =>
                {
                    let pending = ledger.get(&registration.operation)?.is_some_and(|record| {
                        !record.state.terminal()
                            && record.cancel_requested
                            && !record.cancel_dispatched
                            && service
                                .ext::<Arc<GraphNodeRegistry>>()
                                .step(&record.request.node_type)
                                .is_some_and(|step| {
                                    step.plugin == record.plugin
                                        && step.descriptor.revision == record.request.revision
                                        && step.descriptor.lifecycle.cancel
                                })
                    });
                    (!pending).then_some(MonitorStop::OwnerStopped)
                }
                _ => None,
            }
        } else {
            None
        };
        if let Some(reason) = stopped {
            suspend(service, handle, registration, reason)?;
        }
    }
    Ok(())
}

pub(crate) fn reconcile(service: &ServiceState, handle: &Handle) -> Result<Option<u64>, String> {
    prepare_monitors(service, handle)?;
    let ledger = plugin::ledger(service)?;
    let now = handle.store.lock().clock.now_ms();
    for _ in 0..4 {
        let Some(claim) = ledger.claim_job_check(now)? else {
            break;
        };
        let service = service.clone();
        let handle = handle.clone();
        codeterm_util::named_thread::spawn("plugin-job-check", move || {
            if let Err(error) = run_check(&service, &handle, claim) {
                log::debug!("[blueprint] plugin job check unavailable: {error}");
            }
            handle.notify();
        });
    }
    ledger.next_job_check()
}

fn run_check(service: &ServiceContext, handle: &Handle, claim: MonitorClaim) -> Result<(), String> {
    let registration = claim.monitor.registration.clone();
    if let Err(error) = check(service, handle, claim) {
        let reason = plugin::ledger(service)?
            .job_monitor(&registration.operation)?
            .and_then(|monitor| monitor.stopped)
            .unwrap_or_else(|| {
                if live(service, &registration.scope) {
                    MonitorStop::PluginUnavailable
                } else {
                    MonitorStop::ScopeRemoved
                }
            });
        suspend(service, handle, &registration, reason)?;
        return Err(error);
    }
    Ok(())
}

fn sync_hold(
    handle: &Handle,
    registration: &PluginJobRegistration,
    state: OperationState,
    reason: &str,
) -> Result<(), String> {
    let PluginJobOrigin::Blueprint {
        blueprint,
        run,
        node,
        ..
    } = &registration.origin
    else {
        return Ok(());
    };
    storage::commit(&handle.store, |store| {
        let Some(mut job) = store.get(blueprint).cloned() else {
            return storage::Staged::ready(Ok(false));
        };
        if job.workspace_id != registration.scope.workspace
            || job.team_id != registration.scope.team
            || job.creator_tab_id != registration.scope.owner
        {
            return storage::Staged::ready(Ok(false));
        }
        let Some(held) = job.admission.plugin_hold.as_mut().filter(|held| {
            held.operation == registration.operation && &held.run == run && &held.node == node
        }) else {
            return storage::Staged::ready(Ok(false));
        };
        held.state = serde_json::to_value(state)
            .ok()
            .and_then(|state| state.as_str().map(str::to_owned))
            .unwrap_or_else(|| "unknown".into());
        held.reason = reason.into();
        store.stage_admission(blueprint, job.admission)
    })?;
    Ok(())
}

fn suspend(
    service: &ServiceContext,
    handle: &Handle,
    registration: &PluginJobRegistration,
    reason: MonitorStop,
) -> Result<(), String> {
    let ledger = plugin::ledger(service)?;
    if let Some(record) = ledger.get(&registration.operation)? {
        if record.plugin != registration.scope.plugin
            || record.request.context.scope.workspace != registration.scope.workspace
            || record.request.context.scope.team != registration.scope.team
            || record.request.context.owner != registration.scope.owner
        {
            return Err("plugin monitor operation scope changed".into());
        }
        ledger.mark_uncertain(
            &registration.operation,
            &record.request.context.scope,
            &record.request.context.owner,
        )?;
    }
    ledger.stop_job_monitor(&registration.operation, &registration.scope, reason)?;
    sync_hold(
        handle,
        registration,
        OperationState::Unknown,
        stop_reason(reason),
    )?;
    service.invalidate_plugin_owner_views(&ViewOwnerScope {
        plugin: registration.scope.plugin.clone(),
        workspace: registration.scope.workspace.clone(),
        team: registration.scope.team.clone(),
        owner: registration.scope.owner.clone(),
    })?;
    Ok(())
}

fn stop_reason(reason: MonitorStop) -> &'static str {
    match reason {
        MonitorStop::OwnerStopped => "plugin_step_stopped",
        MonitorStop::ScopeRemoved => "plugin_step_scope_removed",
        MonitorStop::Deadline | MonitorStop::CheckLimit => "plugin_step_observation_expired",
        MonitorStop::PluginUnavailable => "plugin_step_unavailable",
    }
}

fn check(service: &ServiceContext, handle: &Handle, claim: MonitorClaim) -> Result<(), String> {
    let ledger = plugin::ledger(service)?;
    let registration = &claim.monitor.registration;
    if !live(service, &registration.scope) {
        return Err("plugin job scope removed".into());
    }
    let remaining = registration
        .deadline_ms
        .saturating_sub(handle.store.lock().clock.now_ms());
    if remaining == 0 {
        return Err("plugin monitor deadline reached".into());
    }
    let deadline = Instant::now() + Duration::from_millis(remaining.min(30_000));
    let _physical = codeterm_decision::physical::RequestScope::enter(Arc::new(CheckObserver {
        deadline,
        remaining: AtomicUsize::new(8),
        service: service.weak_self(),
        scope: registration.scope.clone(),
    }));
    let state = if matches!(registration.origin, PluginJobOrigin::Blueprint { .. }) {
        let record = ledger
            .get(&registration.operation)?
            .ok_or("plugin operation unavailable")?;
        if record.job.as_deref() != Some(&registration.job)
            || record.plugin != registration.scope.plugin
        {
            return Err("plugin job identity changed".into());
        }
        let registered = service
            .ext::<Arc<GraphNodeRegistry>>()
            .step(&record.request.node_type)
            .ok_or("plugin lifecycle unavailable")?;
        if registered.plugin != record.plugin
            || registered.descriptor.revision != record.request.revision
        {
            return Err("plugin lifecycle revision changed".into());
        }
        let media = plugin::media(service)?;
        let runtime = StepRuntime {
            ledger: &ledger,
            descriptor: &registered.descriptor,
            adapter: registered.adapter.clone(),
            media: Some(&media),
        };
        (if record.cancel_requested && !record.cancel_dispatched {
            runtime.cancel(&record.request, deadline)
        } else {
            runtime.reconcile(&record.request, deadline, Default::default())
        })
        .map_or(OperationState::Unknown, |record| record.state)
    } else {
        let registry = service.ext::<codeterm_plugin::view_host::ViewRegistryHandle>();
        let provider = registry
            .get(&registration.scope.plugin)
            .ok_or("plugin lifecycle unavailable")?;
        let capability = provider
            .job_lifecycle()
            .filter(|descriptor| descriptor.valid())
            .ok_or("plugin lifecycle unavailable")?;
        let epoch = registry
            .registration_id(&registration.scope.plugin)
            .ok_or("plugin lifecycle unavailable")?;
        let call = codeterm_plugin::plugin_jobs::JobCall {
            context: codeterm_plugin::plugin_jobs::JobContext {
                scope: registration.scope.clone(),
                origin: registration.origin.clone(),
                operation: registration.operation.clone(),
                job: registration.job.clone(),
            },
            deadline,
            cancellation: Default::default(),
        };
        let result = if claim.cancel && !capability.cancel {
            Ok(codeterm_plugin::plugin_jobs::JobObservation {
                job: registration.job.clone(),
                state: OperationState::Unknown,
            })
        } else if claim.cancel {
            provider.cancel_job(&call)
        } else {
            provider.reconcile_job(&call)
        };
        if registry
            .registration_id(&registration.scope.plugin)
            .as_deref()
            != Some(epoch.as_str())
        {
            OperationState::Unknown
        } else {
            result
                .ok()
                .filter(|result| result.valid_for(&registration.job))
                .map_or(OperationState::Unknown, |result| result.state)
        }
    };
    if !live(service, &registration.scope) {
        suspend(service, handle, registration, MonitorStop::ScopeRemoved)?;
        return Ok(());
    }
    let now = handle.store.lock().clock.now_ms();
    let job = registration.job.clone();
    let completed = ledger.settle_job_check(claim, &job, state, now, 5000)?;
    if !completed.state.terminal() {
        sync_hold(
            handle,
            &completed.registration,
            completed.state,
            match completed.state {
                OperationState::Queued => "plugin_step_queued",
                OperationState::Running => "plugin_step_running",
                _ => "plugin_step_unknown",
            },
        )?;
    }
    service.invalidate_plugin_owner_views(&ViewOwnerScope {
        plugin: completed.registration.scope.plugin.clone(),
        workspace: completed.registration.scope.workspace.clone(),
        team: completed.registration.scope.team.clone(),
        owner: completed.registration.scope.owner.clone(),
    })?;
    if completed.state.terminal() {
        if let PluginJobOrigin::Blueprint { blueprint, .. } = &completed.registration.origin {
            resume(handle, blueprint, &completed.registration.operation)?;
        }
    }
    Ok(())
}

pub(crate) fn resume(handle: &Handle, blueprint: &str, operation: &str) -> Result<(), String> {
    storage::commit(&handle.store, |store| {
        let Some(mut job) = store.get(blueprint).cloned() else {
            return storage::Staged::ready(Ok(false));
        };
        if job.machine.stopped
            || job
                .admission
                .plugin_hold
                .as_ref()
                .is_none_or(|held| held.operation != operation)
        {
            return storage::Staged::ready(Ok(false));
        }
        job.admission.plugin_hold = None;
        store.stage_admission(blueprint, job.admission)
    })?;
    Ok(())
}

#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod origin_tests;
#[cfg(test)]
mod tests;
