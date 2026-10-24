mod checkpoint;
mod code;
mod dispatch;
mod execution;
pub(crate) use checkpoint::retained_inputs;
mod sources;
use codeterm_blueprint_types::blueprint_execution::ExecutionEnvelope;
use codeterm_blueprint_types::blueprint_journal::{
    JournalMarker, JournalOutcome, JournalReason, JournalState,
};
pub use execution::{execute_step, RunCtx, StepExecution};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use codeterm_blueprint_types::blueprint_pipeline::{
    self as blueprint_pipeline, BlueprintSpec, ResultTarget, RunStatus, RunnerState, Step,
    StepJournalEntry, StepKind, StepOutcome,
};

use super::machine::{Route, Verdict};
use super::probe;
use super::provenance::Classification;
use super::scheduler::Handle;
use super::storage;
use super::store::{Lease, Sentinel};
use codeterm_blueprint_types::sentinel_input::{InputRun, InputSpec, MediaKind};
use codeterm_service_core::service::ServiceContext;
use codeterm_storage::BlueprintJournalWrite;

pub(crate) struct StepResult {
    pub(crate) outcome: StepOutcome,
    pub(crate) stored_outcome: JournalOutcome,
    pub(crate) state: JournalState,
    pub(crate) summary: String,
    pub(crate) reason: Option<JournalReason>,
    pub(crate) next_text: String,
    pub(crate) decision: Option<(JournalOutcome, Option<JournalReason>)>,
    pub(crate) uncertain: bool,
}

fn effect_after_claim<T>(claim: impl FnOnce() -> bool, effect: impl FnOnce() -> T) -> Option<T> {
    if claim() {
        Some(effect())
    } else {
        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RunCompletion {
    state: JournalState,
    reason: JournalReason,
}

impl RunCompletion {
    fn new(state: JournalState, reason: impl Into<JournalReason>) -> Self {
        Self {
            state,
            reason: reason.into(),
        }
    }
    fn held(reason: JournalReason) -> Self {
        Self::new(JournalState::Held, reason)
    }
}

fn required_journal(written: bool) -> Result<(), JournalReason> {
    written
        .then_some(())
        .ok_or(JournalReason::JournalWriteFailed)
}

fn with_run_close(
    execute: impl FnOnce() -> Result<RunCompletion, JournalReason>,
    close: impl FnOnce(&RunCompletion),
) {
    let completion =
        execute().unwrap_or_else(|reason| RunCompletion::new(JournalState::Failed, reason));
    close(&completion);
}

pub(super) fn tick(handle: &Handle, svc: &ServiceContext, lease: &Lease) {
    let run_id = lease
        .job
        .admission
        .continuation
        .as_ref()
        .and_then(|saved| saved.get("run_id"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
    let stop_command = std::cell::Cell::new(false);
    with_run_close(
        || run_pipeline(handle, svc, lease, &run_id, &stop_command),
        |completion| finish_run(handle, lease, &run_id, completion, stop_command.get()),
    );
}

fn finish_run(
    handle: &Handle,
    lease: &Lease,
    run_id: &str,
    completion: &RunCompletion,
    stop_command: bool,
) {
    let closed = write_journal(
        handle,
        &lease.job,
        BlueprintJournalWrite::CloseRun {
            run_id: run_id.into(),
            state: completion.state,
            outcome: completion.state.as_str().into(),
            at_ms: now(handle),
            reason: Some(completion.reason.clone()),
        },
    );
    if completion.reason.as_str() == "plan_limit_locked" {
        handle.notify();
        return;
    }
    if handle.store.lock().get(&lease.job.id).is_none_or(|job| {
        !job.same_owner(&lease.job) || job.definition_version != lease.job.definition_version
    }) {
        handle.notify();
        return;
    }
    checkpoint::clear(handle, &lease.job);
    if stop_command {
        let _ = storage::commit(&handle.store, |store| {
            store.stage_stop(&lease.job.id, "command_failed")
        });
    } else if completion.state == JournalState::Held
        || completion.reason == JournalReason::JournalWriteFailed
        || !closed
    {
        let _ = storage::commit(&handle.store, |store| {
            let Some(mut job) = store.get(&lease.job.id).cloned() else {
                return super::storage::Staged::ready(Ok(false));
            };
            job.machine.held = Some(completion.reason.as_str().into());
            job.last_reason = Some(completion.reason.as_str().into());
            store.stage_finish(&job)
        });
    }
    handle.notify();
}

fn stops_failed_command(completion: &RunCompletion) -> bool {
    completion.state == JournalState::Failed && completion.reason.as_str() == "command_failed"
}

struct PreparedInput {
    text: String,
    cleanup: Option<std::sync::Arc<super::input::Cleanup>>,
    input_source: Option<std::path::PathBuf>,
    input_failure: Option<String>,
    input_kind: Option<MediaKind>,
    input_display: Option<String>,
    input_run: Option<InputRun>,
}

fn prepare_input(
    handle: &Handle,
    lease: &Lease,
    spec: &BlueprintSpec,
    run_id: &str,
) -> PreparedInput {
    let mut text = lease.event.as_ref().map_or_else(String::new, |event| {
        serde_json::to_string(&event.payload).unwrap_or_default()
    });
    let input = match &spec.trigger {
        codeterm_blueprint_types::blueprint_pipeline::Trigger::Input { kind, source } => {
            let mut state = InputRun::Pending;
            let _ = state.advance(InputRun::Materializing);
            let spec = InputSpec {
                kind: *kind,
                data: source.clone(),
                convert_to: None,
                convert_timeout_ms: None,
            };
            let result = super::input::prepare(
                &codeterm_service_core::paths::data_dir(),
                &lease.job.id,
                &run_id,
                &spec,
                input_deadline(handle, lease),
            );
            Some((*kind, source.clone(), result, state))
        }
        _ => None,
    };
    let cleanup = input.as_ref().and_then(|_| {
        codeterm_blueprint_types::sentinel_input::run_dir(
            &codeterm_service_core::paths::data_dir(),
            &lease.job.id,
            &run_id,
        )
        .ok()
        .map(super::input::Cleanup::new)
        .map(std::sync::Arc::new)
    });
    let input_source = input
        .as_ref()
        .and_then(|(_, _, result, _)| result.as_ref().ok().map(|files| files.source.clone()));
    let mut input_failure = input
        .as_ref()
        .and_then(|(_, _, result, _)| result.as_ref().err().cloned());
    let input_kind = input.as_ref().map(|(kind, _, _, _)| *kind);
    if input_kind == Some(MediaKind::Text) {
        if let Some(path) = input_source.as_deref() {
            match std::fs::metadata(path) {
                Ok(metadata)
                    if metadata.len()
                        <= codeterm_blueprint_types::sentinel_input::MAX_CONVERTED_TEXT_BYTES
                            as u64 =>
                {
                    match std::fs::read_to_string(path) {
                        Ok(input_text) => text = input_text,
                        Err(error) => input_failure = Some(format!("input_text_invalid: {error}")),
                    }
                }
                Ok(_) => input_failure = Some("input_text_oversize".into()),
                Err(error) => input_failure = Some(format!("input_text_read_failed: {error}")),
            }
        }
    }
    let input_display = input
        .as_ref()
        .map(|(_, source, _, _)| codeterm_blueprint_types::sentinel_input::display_name(source));
    let input_run = input.as_ref().map(|(_, _, _, state)| state.clone());
    PreparedInput {
        text,
        cleanup,
        input_source,
        input_failure,
        input_kind,
        input_display,
        input_run,
    }
}

fn run_pipeline(
    handle: &Handle,
    svc: &ServiceContext,
    lease: &Lease,
    run_id: &str,
    stop_command: &std::cell::Cell<bool>,
) -> Result<RunCompletion, JournalReason> {
    let Some(spec) = lease.job.pipeline() else {
        return Err(JournalReason::Other("pipeline_missing".into()));
    };
    if !super::routes::owner_live(svc, &lease.job) {
        record_run_stop(handle, lease, run_id, "creator_gone", "skip:creator_gone");
        return Ok(RunCompletion::new(JournalState::Skipped, "creator_gone"));
    }
    let Ok(mut state) = RunnerState::start(spec) else {
        record_run_stop(
            handle,
            lease,
            run_id,
            "invalid_pipeline",
            "skip:invalid_pipeline",
        );
        return Ok(RunCompletion::new(
            JournalState::Skipped,
            "invalid_pipeline",
        ));
    };
    let saved = checkpoint::load(&lease.job)?;
    if let Some(saved) = &saved {
        state.current_step = saved.current_step;
        state.status = if saved.failed {
            RunStatus::Failed
        } else {
            RunStatus::Running
        };
    }
    let PreparedInput {
        mut text,
        cleanup,
        input_source,
        input_failure,
        input_kind,
        input_display,
        mut input_run,
    } = match &saved {
        Some(saved) => saved.prepared_input(),
        None => prepare_input(handle, lease, spec, run_id),
    };
    let mut envelope = if let Some(kind) = input_kind.filter(|kind| *kind != MediaKind::Text) {
        ExecutionEnvelope {
            media: kind.as_str().into(),
            schema_id: None,
            value: serde_json::Value::String(
                input_source
                    .as_ref()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ),
            text: String::new(),
        }
    } else if input_kind == Some(MediaKind::Text) {
        ExecutionEnvelope::from_trigger_text(
            &text,
            spec.steps.first().and_then(|step| step.execution.as_ref()),
        )
    } else {
        ExecutionEnvelope::from_text(&text)
    };
    if let Some(port) = spec
        .steps
        .first()
        .and_then(|step| step.execution.as_ref())
        .and_then(|contract| contract.input.as_ref())
    {
        envelope.schema_id = port.reference.clone();
    }
    let mut ctx = RunCtx::new(handle, svc, lease, run_id);
    ctx.input_kind = input_kind;
    ctx.input_source = input_source;
    ctx.input_failure = input_failure;
    ctx.input_display = input_display.unwrap_or_default();
    ctx.cleanup = cleanup.clone();
    if let Some(saved) = &saved {
        envelope = saved.envelope.clone();
        ctx.outputs = saved.outputs.clone();
        ctx.next_marker = saved.next_marker;
    }
    let mut terminal_failure = None;
    while let Some(index) = state.current_step {
        if lease.cancelled.load(Ordering::SeqCst) {
            ctx.record_skipped(&spec.steps[index].name, "skip:lease_cancelled".into())?;
            return Ok(RunCompletion::new("skipped".into(), "skip:lease_cancelled"));
        }
        if crate::plan_lock::refuse(svc, &lease.job).is_err()
            || handle
                .store
                .lock()
                .get(&lease.job.id)
                .is_some_and(|job| job.revision != lease.job.revision)
        {
            checkpoint::hold(&ctx, &state, &envelope, &text, input_run.as_ref())?;
            return Ok(RunCompletion::held("plan_limit_locked".into()));
        }
        let step = &spec.steps[index];
        if matches!(&step.kind, StepKind::Convert { .. }) {
            if let Some(run) = input_run.as_mut() {
                let _ = run.advance(InputRun::Converting);
            }
        }
        let result = match execute_step(&mut ctx, step, envelope) {
            Ok(result) => result,
            Err(reason) => {
                let held = matches!(
                    reason,
                    JournalReason::UncertainClaim
                        | JournalReason::UncertainEffect
                        | JournalReason::JournalWriteFailed
                );
                return Ok(RunCompletion::new(
                    if held {
                        JournalState::Held
                    } else if reason.as_str().starts_with("skip:") {
                        JournalState::Skipped
                    } else {
                        JournalState::Failed
                    },
                    reason,
                ));
            }
        };
        if result.state == JournalState::Failed {
            if let (Some(run), Some(reason)) = (input_run.as_mut(), result.reason.as_ref()) {
                let _ = run.advance(InputRun::Failed(reason.as_str().into()));
            }
        }
        if result.reason.as_deref() == Some("probe_owner_capacity") {
            let _ = storage::commit(&handle.store, |store| {
                store.stage_stop(&lease.job.id, "probe_owner_capacity")
            });
            handle.notify();
            return Ok(RunCompletion::new(
                JournalState::Failed,
                "probe_owner_capacity",
            ));
        }
        text = result.output.payload_text();
        envelope = result.output;
        let prior_index = index;
        let next = match blueprint_pipeline::advance(&state, result.outcome) {
            Ok(next) => next,
            Err(_) => {
                return Ok(RunCompletion::new("failed".into(), "runner_advance_failed"));
            }
        };
        if let Some(target) = next.current_step.filter(|target| *target > prior_index + 1) {
            for skipped in prior_index + 1..target {
                ctx.record_skipped(
                    &spec.steps[skipped].name,
                    format!("skip:branch to {}", spec.steps[target].name),
                )?;
            }
        }
        if next.status == RunStatus::Failed {
            let completion = RunCompletion::new(
                JournalState::Failed,
                result.reason.clone().unwrap_or_else(|| "failed".into()),
            );
            stop_command.set(stops_failed_command(&completion));
            terminal_failure = result.reason;
        }
        state = next;
    }

    if crate::plan_lock::refuse(svc, &lease.job).is_err()
        || handle
            .store
            .lock()
            .get(&lease.job.id)
            .is_some_and(|job| job.revision != lease.job.revision)
    {
        checkpoint::hold(&ctx, &state, &envelope, &text, input_run.as_ref())?;
        return Ok(RunCompletion::held("plan_limit_locked".into()));
    }
    let terminal_state = if state.status == RunStatus::Failed {
        "failed"
    } else {
        "done"
    };
    let outcome = if terminal_state == "failed" {
        "failed"
    } else {
        "done"
    };
    let verdict = Verdict {
        bucket: outcome.into(),
        confidence: 1.0,
    };
    if let Some(cleanup) = cleanup.as_ref() {
        cleanup.settle();
    }
    let body_target = super::routes::pipeline_result_owner(svc, &lease.job, spec);
    let (body_path, body_ref) = match super::routes::write_pipeline_result_body(
        svc,
        &body_target,
        &lease.job,
        &verdict,
        &text,
    ) {
        Ok(body) => body,
        Err(_) => {
            if claim_step(
                handle,
                lease,
                &run_id,
                JournalMarker::Result,
                "$result",
                now(handle),
            ) {
                let _ = write_journal(
                    handle,
                    &lease.job,
                    BlueprintJournalWrite::Finish {
                        run_id: run_id.to_string(),
                        marker: JournalMarker::Result,
                        state: "failed".into(),
                        outcome: "handler_failed".into(),
                        ended_at_ms: now(handle),
                        result_summary: "result body unavailable".into(),
                        body_ref: None,
                        reason: Some("result_body_write_failed".into()),
                    },
                );
            }
            return Ok(RunCompletion::new(
                "failed".into(),
                "result_body_write_failed",
            ));
        }
    };
    let routes = match result_routes(svc, &lease.job, spec) {
        Ok(routes) => routes,
        Err(_) => {
            if claim_step(
                handle,
                lease,
                &run_id,
                JournalMarker::Result,
                "$result",
                now(handle),
            ) {
                let _ = write_journal(
                    handle,
                    &lease.job,
                    unresolved_target_finish(&run_id, now(handle), &body_ref),
                );
            }
            return Ok(RunCompletion::new(
                "failed".into(),
                "result_target_unresolved",
            ));
        }
    };
    for route in routes {
        if crate::plan_lock::refuse(svc, &lease.job).is_err() {
            checkpoint::hold(&ctx, &state, &envelope, &text, input_run.as_ref())?;
            return Ok(RunCompletion::held("plan_limit_locked".into()));
        }
        if lease.cancelled.load(Ordering::SeqCst) || !super::routes::owner_live(svc, &lease.job) {
            record_result_failure(handle, lease, &run_id, &body_ref, "result_source_fenced");
            return Ok(RunCompletion::new(
                JournalState::Failed,
                "result_source_fenced",
            ));
        }
        if !claim_step(
            handle,
            lease,
            &run_id,
            JournalMarker::Result,
            "$result",
            now(handle),
        ) {
            return Ok(RunCompletion::held(JournalReason::UncertainClaim));
        }
        let result = match &route {
            Route::Notify => super::routes::notify_pipeline_result(
                svc,
                &lease.job,
                &verdict,
                &body_path,
                body_ref.clone(),
            ),
            Route::Send(target) => super::routes::deliver_pipeline_result(
                svc,
                target,
                &lease.job,
                &verdict,
                &body_path,
                body_ref.clone(),
                &run_id,
            ),
            _ => super::routes::PipelineResultDelivery {
                state: "failed",
                reason: "unsupported_result_target",
                body_ref: Some(body_ref.clone()),
            },
        };
        let ended = now(handle);
        let decision = match result.state {
            "delivered" => "handler_result",
            _ => "handler_failed",
        };
        let journal_state = if result.state == "delivered" {
            "done"
        } else {
            "failed"
        };
        let summary = format!("result {}", result.state);
        let finish = BlueprintJournalWrite::Finish {
            run_id: run_id.to_string(),
            marker: JournalMarker::Result,
            state: journal_state.into(),
            outcome: decision.into(),
            ended_at_ms: ended,
            result_summary: summary.clone(),
            body_ref: result.body_ref.clone(),
            reason: Some(result.reason.into()),
        };
        if !write_journal(handle, &lease.job, finish) {
            let _ = write_journal(
                handle,
                &lease.job,
                BlueprintJournalWrite::Decision {
                    run_id: run_id.to_string(),
                    marker: JournalMarker::Result,
                    step_name: "$result".into(),
                    outcome: "handler_unconfirmed".into(),
                    at_ms: ended,
                    result_summary: Some(summary),
                    reason: Some("result_journal_finish_failed".into()),
                },
            );
            return Err(JournalReason::JournalWriteFailed);
        }
        if result.state != "delivered" {
            return Ok(RunCompletion::new("failed".into(), result.reason));
        }
    }
    if terminal_state == "done" {
        if let Some(run) = input_run.as_mut() {
            let _ = run.advance(InputRun::Delivered);
        }
    }
    Ok(RunCompletion::new(
        terminal_state.into(),
        terminal_failure.unwrap_or_else(|| outcome.into()),
    ))
}

fn command_failure(failure: probe::process::ProbeFailure, previous: &str) -> StepResult {
    let uncertain = failure.is_unconfirmed();
    let reason = if failure.is_deadline() {
        JournalReason::Deadline
    } else {
        failure.reason().into()
    };
    StepResult {
        outcome: StepOutcome::Failed,
        stored_outcome: "failed".into(),
        state: JournalState::Failed,
        summary: if uncertain {
            "command completion was unconfirmed"
        } else {
            "command probe failed"
        }
        .into(),
        reason: Some(reason.clone()),
        next_text: previous.into(),
        decision: (!uncertain).then(|| ("probe_error".into(), Some(reason))),
        uncertain,
    }
}

fn failed_input_step(reason: &str) -> StepResult {
    StepResult {
        outcome: StepOutcome::Failed,
        stored_outcome: "failed".into(),
        state: JournalState::Failed,
        summary: format!("input materialization failed: {reason}"),
        reason: Some(reason.into()),
        next_text: format!("Input materialization failed: {reason}"),
        decision: None,
        uncertain: false,
    }
}

fn input_deadline(handle: &Handle, lease: &Lease) -> Instant {
    let default = Instant::now()
        + Duration::from_millis(
            codeterm_blueprint_types::sentinel_input::DEFAULT_CONVERT_TIMEOUT_MS,
        );
    lease
        .job
        .expires_at
        .map(|at| Instant::now() + Duration::from_millis(at.saturating_sub(now(handle))))
        .map_or(default, |expiry| expiry.min(default))
}

fn classify_step(classification: Classification, buckets: &[String], previous: &str) -> StepResult {
    match classification.result {
        Ok(verdict) if verdict.confidence >= 0.80 && buckets.contains(&verdict.bucket) => {
            StepResult {
                outcome: StepOutcome::Bucket(verdict.bucket.clone()),
                stored_outcome: verdict.bucket.clone().into(),
                state: JournalState::Done,
                summary: format!("classified {}", verdict.bucket),
                reason: None,
                next_text: previous.into(),
                decision: Some(("classified".into(), None)),
                uncertain: false,
            }
        }
        Ok(verdict) if verdict.confidence < 0.80 => StepResult {
            outcome: StepOutcome::Bucket("other".into()),
            stored_outcome: "other".into(),
            state: JournalState::Done,
            summary: "classification confidence below threshold".into(),
            reason: Some("low_confidence".into()),
            next_text: previous.into(),
            decision: Some(("low_confidence".into(), Some("low_confidence".into()))),
            uncertain: false,
        },
        Ok(_) => StepResult {
            outcome: StepOutcome::Bucket("other".into()),
            stored_outcome: "other".into(),
            state: JournalState::Done,
            summary: "no declared bucket matched".into(),
            reason: Some("other".into()),
            next_text: previous.into(),
            decision: Some(("other".into(), Some("other".into()))),
            uncertain: false,
        },
        Err(failure) => StepResult {
            outcome: StepOutcome::Bucket("other".into()),
            stored_outcome: "other".into(),
            state: JournalState::Done,
            summary: "classification unavailable".into(),
            reason: Some("probe_error".into()),
            next_text: previous.into(),
            decision: Some(("probe_error".into(), Some(failure.reason().into()))),
            uncertain: false,
        },
    }
}

fn result_routes(
    svc: &ServiceContext,
    job: &Sentinel,
    spec: &BlueprintSpec,
) -> Result<Vec<Route>, String> {
    match &spec.result.to {
        ResultTarget::Owner
            if spec.owner == codeterm_blueprint_types::blueprint_pipeline::BlueprintOwner::Team =>
        {
            team_routes(svc, job)
        }
        ResultTarget::Owner => Ok(vec![Route::Send(job.creator_tab_id.clone())]),
        ResultTarget::Notify => Ok(vec![Route::Notify]),
        ResultTarget::Tab(id) => {
            let resolved = super::routes::resolve_send(&svc.mux.read(), job, id)?;
            Ok(vec![resolved])
        }
        ResultTarget::Team => team_routes(svc, job),
    }
}

fn team_routes(svc: &ServiceContext, job: &Sentinel) -> Result<Vec<Route>, String> {
    let mux = svc.mux.read();
    let team_id = job.team_id.as_deref().ok_or("result_team_missing")?;
    let recipient = mux
        .all_tabs()
        .find(|tab| {
            tab.agent_type.is_some()
                && tab.anchors_team()
                && tab.team_id.as_deref() == Some(team_id)
                && mux.find_workspace_for_tab(&tab.id).as_deref() == Some(&job.workspace_id)
        })
        .map(|tab| tab.id.clone())
        .ok_or("result_team_orchestrator_unresolved")?;
    Ok(vec![Route::Send(recipient)])
}

fn record_result_failure(
    handle: &Handle,
    lease: &Lease,
    run_id: &str,
    body_ref: &str,
    reason: &str,
) {
    if claim_step(
        handle,
        lease,
        run_id,
        JournalMarker::Result,
        "$result",
        now(handle),
    ) {
        let _ = write_journal(
            handle,
            &lease.job,
            BlueprintJournalWrite::Finish {
                run_id: run_id.into(),
                marker: JournalMarker::Result,
                state: "failed".into(),
                outcome: "handler_failed".into(),
                ended_at_ms: now(handle),
                result_summary: "result delivery failed".into(),
                body_ref: Some(body_ref.into()),
                reason: Some(reason.into()),
            },
        );
    }
}

fn unresolved_target_finish(
    run_id: &str,
    ended_at_ms: u64,
    body_ref: &str,
) -> BlueprintJournalWrite {
    BlueprintJournalWrite::Finish {
        run_id: run_id.into(),
        marker: JournalMarker::Result,
        state: "failed".into(),
        outcome: "handler_failed".into(),
        ended_at_ms,
        result_summary: "result target unresolved".into(),
        body_ref: Some(body_ref.into()),
        reason: Some("result_target_unresolved".into()),
    }
}

fn claim_step(
    handle: &Handle,
    lease: &Lease,
    run_id: &str,
    marker: JournalMarker,
    name: &str,
    now: u64,
) -> bool {
    write_journal(
        handle,
        &lease.job,
        BlueprintJournalWrite::Claim {
            definition_version: Some(lease.job.definition_version),
            parent: None,
            run_id: run_id.into(),
            marker,
            step_name: marker.name(name).into(),
            generation: lease.job.revision,
            started_at_ms: now,
        },
    )
}

pub(crate) fn write_journal(handle: &Handle, job: &Sentinel, write: BlueprintJournalWrite) -> bool {
    let written = storage::commit(&handle.store, |store| {
        store.stage_pipeline_journal(&job.id, write)
    })
    .unwrap_or_else(|error| {
        log::warn!("[blueprint] {} journal write failed: {error}", job.id);
        false
    });
    handle.notify();
    written
}

fn record_run_stop(handle: &Handle, lease: &Lease, run_id: &str, reason: &str, decision: &str) {
    let now = now(handle);
    let _ = write_journal(
        handle,
        &lease.job,
        BlueprintJournalWrite::Decision {
            run_id: run_id.to_string(),
            marker: JournalMarker::Run,
            step_name: "$run".into(),
            outcome: decision.into(),
            at_ms: now,
            result_summary: None,
            reason: Some(reason.into()),
        },
    );
    let _ = storage::commit(&handle.store, |store| {
        store.stage_stop(&lease.job.id, reason)
    });
}

fn now(handle: &Handle) -> u64 {
    handle.store.lock().clock.now_ms()
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod runtime_tests;
