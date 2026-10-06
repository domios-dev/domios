use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Duration;

use codeterm_types::blueprint_pipeline::{
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
use codeterm_storage::BlueprintJournalWrite;

struct StepResult {
    outcome: StepOutcome,
    stored_outcome: String,
    state: &'static str,
    summary: String,
    reason: Option<String>,
    next_text: String,
    decision: Option<(String, Option<String>)>,
    uncertain: bool,
}

fn effect_after_claim<T>(claim: impl FnOnce() -> bool, effect: impl FnOnce() -> T) -> Option<T> {
    if claim() {
        Some(effect())
    } else {
        None
    }
}

pub(super) fn tick(handle: &Handle, svc: &ServiceContext, lease: &Lease) {
    let Some(spec) = lease.job.pipeline.as_ref() else {
        return;
    };
    if !super::routes::owner_live(svc, &lease.job) {
        record_run_stop(handle, lease, "creator_gone", "skip:creator_gone");
        return;
    }
    let run_id = uuid::Uuid::new_v4().simple().to_string();
    let Ok(mut state) = RunnerState::start(spec) else {
        record_run_stop(handle, lease, "invalid_pipeline", "skip:invalid_pipeline");
        return;
    };
    let mut text = String::new();
    while let Some(index) = state.current_step {
        if lease.cancelled.load(Ordering::SeqCst) {
            let now = now(handle);
            let _ = write_journal(
                handle,
                &lease.job,
                BlueprintJournalWrite::Decision {
                    run_id: run_id.clone(),
                    step_index: index as i64,
                    step_name: spec.steps[index].name.clone(),
                    outcome: "skip:lease_cancelled".into(),
                    at_ms: now,
                    result_summary: None,
                    reason: Some("skip:lease_cancelled".into()),
                },
            );
            close_run(handle, lease, &run_id, "skipped", "skip:lease_cancelled");
            return;
        }
        let step = &spec.steps[index];
        let started = now(handle);
        let Some(result) = effect_after_claim(
            || claim_step(handle, lease, &run_id, index as i64, &step.name, started),
            || execute_step(handle, svc, lease, step, &text),
        ) else {
            let _ = storage::commit(&handle.store, |store| {
                store.stage_stop(&lease.job.id, "uncertain_pipeline_claim")
            });
            handle.notify();
            return;
        };
        let ended = now(handle);
        if result.uncertain {
            let _ = write_journal(
                handle,
                &lease.job,
                BlueprintJournalWrite::Decision {
                    run_id: run_id.clone(),
                    step_index: index as i64,
                    step_name: step.name.clone(),
                    outcome: "probe_error".into(),
                    at_ms: ended,
                    result_summary: Some("command completion was unconfirmed".into()),
                    reason: Some("effect_unconfirmed".into()),
                },
            );
            let _ = storage::commit(&handle.store, |store| {
                store.stage_stop(&lease.job.id, "uncertain_pipeline_effect")
            });
            handle.notify();
            return;
        }
        let finish = BlueprintJournalWrite::Finish {
            run_id: run_id.clone(),
            step_index: index as i64,
            state: result.state.into(),
            outcome: result.stored_outcome.clone(),
            ended_at_ms: ended,
            result_summary: StepJournalEntry::bound_summary(&result.summary),
            reason: result.reason.clone(),
        };
        if !write_journal(handle, &lease.job, finish) {
            return;
        }
        if let Some((outcome, reason)) = result.decision {
            let decision = BlueprintJournalWrite::Decision {
                run_id: run_id.clone(),
                step_index: index as i64,
                step_name: step.name.clone(),
                outcome,
                at_ms: ended,
                result_summary: None,
                reason,
            };
            if !write_journal(handle, &lease.job, decision) {
                return;
            }
        }
        if matches!(&step.kind, StepKind::Command { .. }) && result.next_text == text {
            let unchanged = BlueprintJournalWrite::Decision {
                run_id: run_id.clone(),
                step_index: index as i64,
                step_name: step.name.clone(),
                outcome: "unchanged".into(),
                at_ms: ended,
                result_summary: Some("step output was unchanged".into()),
                reason: None,
            };
            if !write_journal(handle, &lease.job, unchanged) {
                return;
            }
        }
        text = result.next_text;
        let prior_index = index;
        let next = match blueprint_pipeline::advance(&state, result.outcome) {
            Ok(next) => next,
            Err(_) => {
                close_run(handle, lease, &run_id, "failed", "runner_advance_failed");
                return;
            }
        };
        if let Some(target) = next.current_step.filter(|target| *target > prior_index + 1) {
            for skipped in prior_index + 1..target {
                let decision = BlueprintJournalWrite::Decision {
                    run_id: run_id.clone(),
                    step_index: skipped as i64,
                    step_name: spec.steps[skipped].name.clone(),
                    outcome: "skip:branch".into(),
                    at_ms: ended,
                    result_summary: None,
                    reason: Some(format!("skip:branch to {}", spec.steps[target].name)),
                };
                if !write_journal(handle, &lease.job, decision) {
                    return;
                }
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
            if claim_step(handle, lease, &run_id, -1, "$result", now(handle)) {
                let _ = write_journal(
                    handle,
                    &lease.job,
                    BlueprintJournalWrite::Finish {
                        run_id: run_id.clone(),
                        step_index: -1,
                        state: "failed".into(),
                        outcome: "handler_failed".into(),
                        ended_at_ms: now(handle),
                        result_summary: "body_ref:unavailable".into(),
                        reason: Some("result_body_write_failed".into()),
                    },
                );
            }
            close_run(handle, lease, &run_id, "failed", "result_body_write_failed");
            return;
        }
    };
    let routes = match result_routes(svc, &lease.job, spec) {
        Ok(routes) => routes,
        Err(_) => {
            if claim_step(handle, lease, &run_id, -1, "$result", now(handle)) {
                let _ = write_journal(
                    handle,
                    &lease.job,
                    BlueprintJournalWrite::Finish {
                        run_id: run_id.clone(),
                        step_index: -1,
                        state: "failed".into(),
                        outcome: "handler_failed".into(),
                        ended_at_ms: now(handle),
                        result_summary: format!("body_ref:{body_ref}"),
                        reason: Some("result_target_unresolved".into()),
                    },
                );
            }
            close_run(handle, lease, &run_id, "failed", "result_target_unresolved");
            return;
        }
    };
    for route in routes {
        if lease.cancelled.load(Ordering::SeqCst) || !super::routes::owner_live(svc, &lease.job) {
            record_result_failure(handle, lease, &run_id, &body_ref, "result_source_fenced");
            return;
        }
        if !claim_step(handle, lease, &run_id, -1, "$result", now(handle)) {
            let _ = storage::commit(&handle.store, |store| {
                store.stage_stop(&lease.job.id, "uncertain_pipeline_claim")
            });
            handle.notify();
            return;
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
        let summary = result
            .body_ref
            .as_deref()
            .map(|path| format!("body_ref:{path}"))
            .unwrap_or_else(|| "body_ref:unavailable".into());
        let finish = BlueprintJournalWrite::Finish {
            run_id: run_id.clone(),
            step_index: -1,
            state: journal_state.into(),
            outcome: decision.into(),
            ended_at_ms: ended,
            result_summary: summary.clone(),
            reason: Some(result.reason.into()),
        };
        if !write_journal(handle, &lease.job, finish) {
            let _ = write_journal(
                handle,
                &lease.job,
                BlueprintJournalWrite::Decision {
                    run_id: run_id.clone(),
                    step_index: -1,
                    step_name: "$result".into(),
                    outcome: "handler_unconfirmed".into(),
                    at_ms: ended,
                    result_summary: Some(summary),
                    reason: Some("result_journal_finish_failed".into()),
                },
            );
            close_run(
                handle,
                lease,
                &run_id,
                "failed",
                "result_journal_finish_failed",
            );
            return;
        }
        if result.state != "delivered" {
            close_run(handle, lease, &run_id, "failed", result.reason);
            return;
        }
    }
    close_run(handle, lease, &run_id, terminal_state, outcome);
}

fn execute_step(
    handle: &Handle,
    svc: &ServiceContext,
    lease: &Lease,
    step: &Step,
    previous: &str,
) -> StepResult {
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
                &probe::environment::Environment::current_instance(),
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
                        state: if success { "done" } else { "failed" },
                        summary,
                        reason: (!success).then(|| "command_failed".into()),
                        next_text: output.text,
                        decision: None,
                        uncertain: false,
                    }
                }
                Err(failure) if failure.is_unconfirmed() => StepResult {
                    outcome: StepOutcome::Failed,
                    stored_outcome: "failed".into(),
                    state: "failed",
                    summary: "command completion was unconfirmed".into(),
                    reason: Some(failure.reason().into()),
                    next_text: previous.into(),
                    decision: None,
                    uncertain: true,
                },
                Err(_) => StepResult {
                    outcome: StepOutcome::Failed,
                    stored_outcome: "failed".into(),
                    state: "failed",
                    summary: "command probe failed".into(),
                    reason: Some("probe_error".into()),
                    next_text: previous.into(),
                    decision: Some(("probe_error".into(), Some("probe_error".into()))),
                    uncertain: false,
                },
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
                    stored_outcome: outcome,
                    state: "done",
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
                state: "failed",
                summary: "filter did not match".into(),
                reason: Some("filter_no_match".into()),
                next_text: previous.into(),
                decision: Some(("filter_no_match".into(), Some("filter_no_match".into()))),
                uncertain: false,
            },
            Err(_) => StepResult {
                outcome: StepOutcome::Failed,
                stored_outcome: "failed".into(),
                state: "failed",
                summary: "filter pattern failed to compile".into(),
                reason: Some("invalid_filter".into()),
                next_text: previous.into(),
                decision: Some(("probe_error".into(), Some("invalid_filter".into()))),
                uncertain: false,
            },
        },
        StepKind::Convert { .. } => StepResult {
            outcome: StepOutcome::Failed,
            stored_outcome: "failed".into(),
            state: "failed",
            summary: "conversion capability is not executable".into(),
            reason: Some("converter_unavailable".into()),
            next_text: previous.into(),
            decision: Some((
                "skip:converter_unavailable".into(),
                Some("converter_unavailable".into()),
            )),
            uncertain: false,
        },
    }
}

fn classify_step(classification: Classification, buckets: &[String], previous: &str) -> StepResult {
    match classification.result {
        Ok(verdict) if verdict.confidence >= 0.80 && buckets.contains(&verdict.bucket) => {
            StepResult {
                outcome: StepOutcome::Bucket(verdict.bucket.clone()),
                stored_outcome: verdict.bucket.clone(),
                state: "done",
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
            state: "done",
            summary: "classification confidence below threshold".into(),
            reason: Some("low_confidence".into()),
            next_text: previous.into(),
            decision: Some(("low_confidence".into(), Some("low_confidence".into()))),
            uncertain: false,
        },
        Ok(_) => StepResult {
            outcome: StepOutcome::Bucket("other".into()),
            stored_outcome: "other".into(),
            state: "done",
            summary: "no declared bucket matched".into(),
            reason: Some("other".into()),
            next_text: previous.into(),
            decision: Some(("other".into(), Some("other".into()))),
            uncertain: false,
        },
        Err(failure) => StepResult {
            outcome: StepOutcome::Bucket("other".into()),
            stored_outcome: "other".into(),
            state: "done",
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
            if spec.owner == codeterm_types::blueprint_pipeline::BlueprintOwner::Team =>
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
    if claim_step(handle, lease, run_id, -1, "$result", now(handle)) {
        let _ = write_journal(
            handle,
            &lease.job,
            BlueprintJournalWrite::Finish {
                run_id: run_id.into(),
                step_index: -1,
                state: "failed".into(),
                outcome: "handler_failed".into(),
                ended_at_ms: now(handle),
                result_summary: format!("body_ref:{body_ref}"),
                reason: Some(reason.into()),
            },
        );
    }
    close_run(handle, lease, run_id, "failed", reason);
}

fn claim_step(
    handle: &Handle,
    lease: &Lease,
    run_id: &str,
    index: i64,
    name: &str,
    now: u64,
) -> bool {
    write_journal(
        handle,
        &lease.job,
        BlueprintJournalWrite::Claim {
            run_id: run_id.into(),
            step_index: index,
            step_name: name.into(),
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

fn close_run(handle: &Handle, lease: &Lease, run_id: &str, state: &str, reason: &str) {
    let now = now(handle);
    let _ = write_journal(
        handle,
        &lease.job,
        BlueprintJournalWrite::CloseRun {
            run_id: run_id.into(),
            state: state.into(),
            outcome: state.into(),
            at_ms: now,
            reason: Some(reason.into()),
        },
    );
}

fn record_run_stop(handle: &Handle, lease: &Lease, reason: &str, decision: &str) {
    let run_id = uuid::Uuid::new_v4().simple().to_string();
    let now = now(handle);
    let _ = write_journal(
        handle,
        &lease.job,
        BlueprintJournalWrite::Decision {
            run_id: run_id.clone(),
            step_index: -1,
            step_name: "$run".into(),
            outcome: decision.into(),
            at_ms: now,
            result_summary: None,
            reason: Some(reason.into()),
        },
    );
    close_run(handle, lease, &run_id, "skipped", reason);
    let _ = storage::commit(&handle.store, |store| {
        store.stage_stop(&lease.job.id, reason)
    });
}

fn now(handle: &Handle) -> u64 {
    handle.store.lock().clock.now_ms()
}

#[cfg(test)]
mod tests;
