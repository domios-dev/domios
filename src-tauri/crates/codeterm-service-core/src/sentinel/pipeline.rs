use codeterm_blueprint_types::blueprint_journal::{
    JournalMarker, JournalOutcome, JournalReason, JournalState,
};
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
use crate::service::ServiceContext;
use codeterm_blueprint_types::sentinel_input::{InputRun, InputSpec, MediaKind};
use codeterm_storage::BlueprintJournalWrite;

struct StepResult {
    outcome: StepOutcome,
    stored_outcome: JournalOutcome,
    state: JournalState,
    summary: String,
    reason: Option<JournalReason>,
    next_text: String,
    decision: Option<(JournalOutcome, Option<JournalReason>)>,
    uncertain: bool,
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
    let run_id = uuid::Uuid::new_v4().simple().to_string();
    with_run_close(
        || run_pipeline(handle, svc, lease, &run_id),
        |completion| finish_run(handle, lease, &run_id, completion),
    );
}

fn finish_run(handle: &Handle, lease: &Lease, run_id: &str, completion: &RunCompletion) {
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
    if completion.state == JournalState::Held
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
                &crate::paths::data_dir(),
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
            &crate::paths::data_dir(),
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
    let PreparedInput {
        mut text,
        cleanup,
        input_source,
        input_failure,
        input_kind,
        input_display,
        mut input_run,
    } = prepare_input(handle, lease, spec, run_id);
    while let Some(index) = state.current_step {
        if lease.cancelled.load(Ordering::SeqCst) {
            let now = now(handle);
            let _ = write_journal(
                handle,
                &lease.job,
                BlueprintJournalWrite::Decision {
                    run_id: run_id.to_string(),
                    marker: JournalMarker::Step(index as u16),
                    step_name: spec.steps[index].name.clone(),
                    outcome: "skip:lease_cancelled".into(),
                    at_ms: now,
                    result_summary: None,
                    reason: Some("skip:lease_cancelled".into()),
                },
            );
            return Ok(RunCompletion::new("skipped".into(), "skip:lease_cancelled"));
        }
        let step = &spec.steps[index];
        if matches!(&step.kind, StepKind::Convert { .. }) {
            if let Some(run) = input_run.as_mut() {
                let _ = run.advance(InputRun::Converting);
            }
        }
        let started = now(handle);
        let Some(result) = effect_after_claim(
            || {
                claim_step(
                    handle,
                    lease,
                    &run_id,
                    JournalMarker::Step(index as u16),
                    &step.name,
                    started,
                )
            },
            || {
                run_step(
                    handle,
                    svc,
                    lease,
                    step,
                    &text,
                    input_kind,
                    input_source.as_deref(),
                    input_failure.as_deref(),
                    input_display.as_deref().unwrap_or_default(),
                    cleanup.clone(),
                )
            },
        ) else {
            return Ok(RunCompletion::held(JournalReason::UncertainClaim));
        };
        let agent_body = if matches!(&step.kind, StepKind::Agent { .. }) {
            Some(
                super::agent_stage::persist_result(run_id, index, &result.next_text)
                    .map_err(|_| JournalReason::Other("agent_result_write_failed".into()))?,
            )
        } else {
            None
        };
        if lease.cancelled.load(Ordering::SeqCst) {
            if matches!(&step.kind, StepKind::Agent { .. }) {
                required_journal(write_journal(
                    handle,
                    &lease.job,
                    BlueprintJournalWrite::Finish {
                        run_id: run_id.into(),
                        marker: JournalMarker::Step(index as u16),
                        state: JournalState::Failed,
                        outcome: "failed".into(),
                        ended_at_ms: now(handle),
                        result_summary: "agent task cancelled".into(),
                        body_ref: agent_body,
                        reason: Some("agent_cancelled".into()),
                    },
                ))?;
                return Ok(RunCompletion::new(JournalState::Failed, "agent_cancelled"));
            }
            if matches!(&step.kind, StepKind::Convert { .. }) {
                let reason = result.reason.as_deref().unwrap_or("conversion_cancelled");
                let _ = write_journal(
                    handle,
                    &lease.job,
                    BlueprintJournalWrite::Finish {
                        run_id: run_id.to_string(),
                        marker: JournalMarker::Step(index as u16),
                        state: "failed".into(),
                        outcome: "failed".into(),
                        ended_at_ms: now(handle),
                        result_summary: format!("conversion failed: {reason}"),
                        body_ref: None,
                        reason: Some(reason.into()),
                    },
                );
                return Ok(RunCompletion::new("failed".into(), reason));
            } else {
                return Ok(RunCompletion::new("skipped".into(), "skip:lease_cancelled"));
            }
        }
        let ended = now(handle);
        if result.uncertain {
            let _ = write_journal(
                handle,
                &lease.job,
                BlueprintJournalWrite::Decision {
                    run_id: run_id.to_string(),
                    marker: JournalMarker::Step(index as u16),
                    step_name: step.name.clone(),
                    outcome: "probe_error".into(),
                    at_ms: ended,
                    result_summary: Some("command completion was unconfirmed".into()),
                    reason: Some(JournalReason::UncertainEffect),
                },
            );
            return Ok(RunCompletion::held(JournalReason::UncertainEffect));
        }
        if result.state == "failed" {
            if let (Some(run), Some(reason)) = (input_run.as_mut(), result.reason.as_ref()) {
                let _ = run.advance(InputRun::Failed(reason.as_str().into()));
            }
        }
        let finish = BlueprintJournalWrite::Finish {
            run_id: run_id.to_string(),
            marker: JournalMarker::Step(index as u16),
            state: result.state.into(),
            outcome: result.stored_outcome.clone().into(),
            ended_at_ms: ended,
            result_summary: StepJournalEntry::bound_summary(&result.summary),
            body_ref: agent_body,
            reason: result.reason.clone(),
        };
        required_journal(write_journal(handle, &lease.job, finish))?;
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
        if let Some((outcome, reason)) = result.decision {
            let decision = BlueprintJournalWrite::Decision {
                run_id: run_id.to_string(),
                marker: JournalMarker::Step(index as u16),
                step_name: step.name.clone(),
                outcome,
                at_ms: ended,
                result_summary: None,
                reason,
            };
            required_journal(write_journal(handle, &lease.job, decision))?;
        }
        if matches!(&step.kind, StepKind::Command { .. }) && result.next_text == text {
            let unchanged = BlueprintJournalWrite::Decision {
                run_id: run_id.to_string(),
                marker: JournalMarker::Step(index as u16),
                step_name: step.name.clone(),
                outcome: "unchanged".into(),
                at_ms: ended,
                result_summary: Some("step output was unchanged".into()),
                reason: None,
            };
            required_journal(write_journal(handle, &lease.job, unchanged))?;
        }
        text = result.next_text;
        let prior_index = index;
        let next = match blueprint_pipeline::advance(&state, result.outcome) {
            Ok(next) => next,
            Err(_) => {
                return Ok(RunCompletion::new("failed".into(), "runner_advance_failed"));
            }
        };
        if let Some(target) = next.current_step.filter(|target| *target > prior_index + 1) {
            for skipped in prior_index + 1..target {
                let decision = BlueprintJournalWrite::Decision {
                    run_id: run_id.to_string(),
                    marker: JournalMarker::Step(skipped as u16),
                    step_name: spec.steps[skipped].name.clone(),
                    outcome: "skip:branch".into(),
                    at_ms: ended,
                    result_summary: None,
                    reason: Some(format!("skip:branch to {}", spec.steps[target].name).into()),
                };
                required_journal(write_journal(handle, &lease.job, decision))?;
            }
        }
        state = next;
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
            "unconfirmed" => "handler_unconfirmed",
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
        if result.state == "unconfirmed" {
            return Ok(RunCompletion::held(JournalReason::UncertainResult));
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
    Ok(RunCompletion::new(terminal_state.into(), outcome))
}

fn run_step(
    handle: &Handle,
    svc: &ServiceContext,
    lease: &Lease,
    step: &Step,
    previous: &str,
    input_kind: Option<MediaKind>,
    input_source: Option<&Path>,
    input_failure: Option<&str>,
    input_display: &str,
    cleanup: Option<std::sync::Arc<super::input::Cleanup>>,
) -> StepResult {
    if let Some(reason) = input_failure {
        return failed_input_step(reason);
    }
    match &step.kind {
        StepKind::Command { command } => {
            let remaining = lease
                .job
                .expires_at
                .map(|at| Duration::from_millis(at.saturating_sub(now(handle))))
                .unwrap_or(probe::PROBE_RUNTIME);
            let result = probe::run_with_environment(
                command,
                Path::new(&lease.job.cwd),
                remaining,
                &handle.probe_owner,
                &super::probe_env::current_instance(svc.configuration().api),
            );
            match result {
                Ok(output) => {
                    let success = output.success;
                    let summary = if success {
                        format!("command completed ({} output bytes)", output.bytes.len())
                    } else {
                        format!("command failed ({} output bytes)", output.bytes.len())
                    };
                    StepResult {
                        outcome: if success {
                            StepOutcome::Done
                        } else {
                            StepOutcome::Failed
                        },
                        stored_outcome: if success { "done" } else { "failed" }.into(),
                        state: if success {
                            JournalState::Done
                        } else {
                            JournalState::Failed
                        },
                        summary,
                        reason: (!success).then(|| "command_failed".into()),
                        next_text: output
                            .stdout_text()
                            .trim_end_matches(&['\n', '\r'][..])
                            .to_string(),
                        decision: None,
                        uncertain: false,
                    }
                }
                Err(failure) => command_failure(failure, previous),
            }
        }
        StepKind::Agent {
            provider,
            profile,
            task,
            deadline_ms,
            max_tokens,
            max_turns,
        } => {
            let remaining = lease
                .job
                .expires_at
                .map(|at| at.saturating_sub(now(handle)));
            let deadline =
                super::agent_stage::effective_deadline(*deadline_ms, remaining, Instant::now());
            let request = codeterm_plugin::agent_task::AgentTaskRequest {
                input: codeterm_plugin::agent_task::AgentTaskInput {
                    run_id: uuid::Uuid::new_v4().simple().to_string(),
                    tab_id: lease.job.creator_tab_id.clone(),
                    workspace_id: lease.job.workspace_id.clone(),
                    profile: profile.clone(),
                    task: task.clone(),
                    input: previous.into(),
                    timeout_ms: deadline
                        .saturating_duration_since(Instant::now())
                        .as_millis()
                        .max(1) as u64,
                    max_tokens: *max_tokens,
                    max_turns: *max_turns,
                },
                deadline,
                cancellation: lease.cancelled.conversion_token(),
            };
            let result = svc
                .chatbackend_registry
                .get(provider)
                .ok_or_else(|| "agent provider unavailable".to_string())
                .and_then(|provider| super::agent_stage::run(provider, request));
            let result =
                result.unwrap_or_else(|error| codeterm_plugin::agent_task::AgentTaskResult {
                    status: codeterm_plugin::agent_task::AgentTaskStatus::Failed,
                    output: serde_json::Value::Null,
                    error: Some(error),
                });
            let (done, _) = super::agent_stage::result_fields(&result);
            StepResult {
                outcome: if done {
                    StepOutcome::Done
                } else {
                    StepOutcome::Failed
                },
                stored_outcome: if done { "done" } else { "failed" }.into(),
                state: if done {
                    JournalState::Done
                } else {
                    JournalState::Failed
                },
                summary: if done {
                    "agent task completed"
                } else {
                    "agent task failed or aborted"
                }
                .into(),
                reason: result.error.clone().map(Into::into),
                next_text: serde_json::to_string(&result).unwrap_or_default(),
                decision: None,
                uncertain: false,
            }
        }
        StepKind::Ask { question, buckets } => {
            let mut classify_job = lease.job.clone();
            classify_job.question = question.clone();
            classify_job.buckets = buckets.clone();
            classify_job.matches.clear();
            let classification =
                super::classify::classify(svc, &classify_job, previous, Duration::from_secs(30));
            classify_step(classification, buckets, previous)
        }
        StepKind::Filter { pattern, bucket } => match regex::Regex::new(pattern) {
            Ok(regex) if regex.is_match(previous) => {
                let outcome = bucket.clone().unwrap_or_else(|| "done".into());
                let step_outcome = if outcome == "done" {
                    StepOutcome::Done
                } else {
                    StepOutcome::Bucket(outcome.clone())
                };
                StepResult {
                    outcome: step_outcome,
                    stored_outcome: outcome.into(),
                    state: JournalState::Done,
                    summary: "filter matched".into(),
                    reason: None,
                    next_text: previous.into(),
                    decision: Some(("filter_match".into(), None)),
                    uncertain: false,
                }
            }
            Ok(_) => StepResult {
                outcome: StepOutcome::Failed,
                stored_outcome: "failed".into(),
                state: JournalState::Failed,
                summary: "filter did not match".into(),
                reason: Some("filter_no_match".into()),
                next_text: previous.into(),
                decision: Some(("filter_no_match".into(), Some("filter_no_match".into()))),
                uncertain: false,
            },
            Err(_) => StepResult {
                outcome: StepOutcome::Failed,
                stored_outcome: "failed".into(),
                state: JournalState::Failed,
                summary: "filter pattern failed to compile".into(),
                reason: Some("invalid_filter".into()),
                next_text: previous.into(),
                decision: Some(("probe_error".into(), Some("invalid_filter".into()))),
                uncertain: false,
            },
        },
        StepKind::Convert { from, to } => {
            let conversion = match (input_kind, input_source) {
                (Some(kind), Some(path)) if kind == *from => super::input::convert(
                    &svc.converter_registry,
                    path,
                    path.parent().unwrap_or_else(|| Path::new(".")),
                    *from,
                    *to,
                    input_deadline(handle, lease),
                    lease.cancelled.conversion_token(),
                    cleanup,
                    Instant::now,
                ),
                (Some(_), Some(_)) => Err("conversion_input_kind_mismatch".into()),
                _ => Err("conversion_input_missing".into()),
            };
            match conversion {
                Ok(output) => StepResult {
                    outcome: StepOutcome::Done,
                    stored_outcome: "done".into(),
                    state: JournalState::Done,
                    summary: format!(
                        "{} {} -> {}, {} chars",
                        from.as_str(),
                        input_display,
                        to.as_str(),
                        output.chars().count()
                    ),
                    reason: None,
                    next_text: output,
                    decision: None,
                    uncertain: false,
                },
                Err(reason) => StepResult {
                    outcome: StepOutcome::Failed,
                    stored_outcome: "failed".into(),
                    state: JournalState::Failed,
                    summary: format!("conversion failed: {reason}"),
                    reason: Some(reason.clone().into()),
                    next_text: format!("Input conversion failed: {reason}"),
                    decision: None,
                    uncertain: false,
                },
            }
        }
    }
}

fn command_failure(failure: probe::process::ProbeFailure, previous: &str) -> StepResult {
    let uncertain = failure.is_unconfirmed();
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
        reason: Some(failure.reason().into()),
        next_text: previous.into(),
        decision: (!uncertain).then(|| ("probe_error".into(), Some(failure.reason().into()))),
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
            run_id: run_id.into(),
            marker,
            step_name: marker.name(name).into(),
            generation: lease.job.revision,
            started_at_ms: now,
        },
    )
}

fn write_journal(handle: &Handle, job: &Sentinel, write: BlueprintJournalWrite) -> bool {
    let written = storage::commit(&handle.store, |store| {
        store.stage_pipeline_journal(&job.id, write)
    })
    .unwrap_or(false);
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
