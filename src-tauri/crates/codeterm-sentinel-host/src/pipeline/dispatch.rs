use super::*;

pub(super) fn run_step(
    handle: &Handle,
    svc: &ServiceContext,
    lease: &Lease,
    step: &Step,
    input: &ExecutionEnvelope,
    input_kind: Option<MediaKind>,
    input_source: Option<&Path>,
    input_failure: Option<&str>,
    input_display: &str,
    cleanup: Option<std::sync::Arc<crate::input::Cleanup>>,
    limit: Duration,
    run_id: &str,
) -> StepResult {
    let previous_text = input.payload_text();
    let previous = previous_text.as_str();
    if let Some(reason) = input_failure {
        return failed_input_step(reason);
    }
    match &step.kind {
        StepKind::Call { .. } | StepKind::Effect { .. } | StepKind::Plugin { .. } => {
            failed_input_step("call_context_required")
        }
        StepKind::Code { code } => super::code::run(handle, lease, step, code, input, limit),
        StepKind::Command { .. } | StepKind::Github { .. } | StepKind::Http { .. } => {
            sources::run(handle, lease, step, input, limit)
        }
        StepKind::Agent {
            review,
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
                .map(|at| {
                    at.saturating_sub(now(handle))
                        .min(limit.as_millis().min(u64::MAX as u128) as u64)
                })
                .or(Some(limit.as_millis().min(u64::MAX as u128) as u64));
            let deadline =
                crate::agent_stage::effective_deadline(*deadline_ms, remaining, Instant::now());
            let request = codeterm_plugin::agent_task::AgentTaskRequest {
                input: codeterm_plugin::agent_task::AgentTaskInput {
                    run_id: run_id.into(),
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
                .and_then(|provider| crate::agent_stage::run(provider, request));
            let result =
                result.unwrap_or_else(|error| codeterm_plugin::agent_task::AgentTaskResult {
                    status: codeterm_plugin::agent_task::AgentTaskStatus::Failed,
                    output: serde_json::Value::Null,
                    error: Some(error),
                });
            let (mut done, _) = crate::agent_stage::result_fields(&result);
            let verdict = if *review && done {
                super::agent_review::review_verdict(&result.output).ok()
            } else {
                None
            };
            if *review && verdict.is_none() {
                done = false;
            }
            let outcome = if done {
                verdict.unwrap_or("done")
            } else {
                "failed"
            };
            StepResult {
                outcome: match outcome {
                    "done" => StepOutcome::Done,
                    "failed" => StepOutcome::Failed,
                    verdict => StepOutcome::Bucket(verdict.into()),
                },
                stored_outcome: outcome.into(),
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
            let classification = crate::classify::classify(
                svc,
                &classify_job,
                previous,
                Duration::from_secs(30).min(limit),
            );
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
                (Some(kind), Some(path)) if kind == *from => crate::input::convert(
                    svc.ext::<std::sync::Arc<codeterm_plugin::converter::ConverterRegistry>>(),
                    path,
                    path.parent().unwrap_or_else(|| Path::new(".")),
                    *from,
                    *to,
                    input_deadline(handle, lease).min(Instant::now() + limit),
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
