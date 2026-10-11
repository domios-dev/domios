use super::*;

pub fn record(
    runtime: &supporter::SupporterRuntime,
    job: &Sentinel,
    outcome: &machine::Outcome,
    stage: Stage,
) -> std::io::Result<()> {
    if !outcome.evidence.matches(job) {
        log::warn!("[blueprint] journal refusal: observation scope or state hash mismatch");
        return Err(known_pre_effect_journal_refusal(
            "blueprint observation scope or state hash mismatch",
        ));
    }
    let route_stage = matches!(
        stage,
        Stage::RouteClaim | Stage::RouteCompleted | Stage::RouteFailed
    );
    if route_stage
        && (!machine::TransitionReason::parse(outcome.reason)
            .is_some_and(machine::TransitionReason::routes_handlers)
            || outcome.route.is_none()
            || outcome.verdict.is_none())
    {
        log::warn!(
            "[blueprint] journal refusal: route stage missing a routable transition reason, route, or verdict; reason={}",
            outcome.reason
        );
        return Err(known_pre_effect_journal_refusal(
            "blueprint route stage requires a confident transition",
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
        "time": chrono::Utc::now().timestamp(), "kind": "blueprint", "blueprint_id": job.id,
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

pub(super) fn record_input_unavailable(handle: &Handle, svc: &ServiceState, lease: &Lease) {
    let mut job = lease.job.clone();
    job.last_reason = Some("input_runtime_unavailable".into());
    if storage::commit(&handle.store, |store| store.stage_finish(&job)).unwrap_or(false) {
        let _ = journal(
            svc,
            &job,
            &machine::Outcome {
                evidence: Evidence::new(Scope::from(&lease.job), None),
                reason: "input_runtime_unavailable",
                verdict: None,
                route: None,
                handlers: Vec::new(),
            },
            Stage::Skip,
        );
    }
}

pub(super) fn record_failure(
    handle: &Handle,
    svc: &ServiceState,
    lease: &Lease,
    failure: &probe::process::ProbeFailure,
    stored_reason: Option<String>,
) {
    let mut job = lease.job.clone();
    job.progress
        .record(handle.store.lock().clock.now_ms(), Some("error"), false);
    job.machine.last_hash = None;
    let removed_tab = failure.reason() == "blueprint_observation_tab_removed";
    job.last_reason = Some(if removed_tab {
        "observed_tab_removed".into()
    } else {
        stored_reason.unwrap_or_else(|| failure.reason())
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

pub(super) fn record_unconfirmed_failure(
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
            "[blueprint] probe stop persistence failed: {error}; resource failure: {}",
            failure.reason()
        );
    }
    if let Some(error) = result.journal_error {
        log::error!(
            "[blueprint] unconfirmed probe skip journal failed: {error}; resource failure: {}",
            failure.reason()
        );
    }
    handle.notify();
}

pub(crate) struct UnconfirmedRecordResult {
    pub persistence_error: Option<String>,
    pub journal_error: Option<String>,
}

pub(crate) fn record_unconfirmed_failure_with(
    store: &Arc<Mutex<Store>>,
    job: &Sentinel,
    failure: &probe::process::ProbeFailure,
    mut write_journal: impl FnMut(&Sentinel, &machine::Outcome, Stage) -> std::io::Result<()>,
) -> UnconfirmedRecordResult {
    let reason = failure.reason();
    let mut failed = job.clone();
    failed
        .progress
        .record(store.lock().clock.now_ms(), Some("error"), false);
    failed.last_reason = Some(reason.clone());
    let _ = storage::commit(store, |current| current.stage_finish(&failed));
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

pub(crate) fn resolve_probe_result<T>(
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
