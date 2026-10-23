use super::*;
use codeterm_blueprint_types::blueprint_github::GithubResource;
use codeterm_blueprint_types::blueprint_source::SourceReading;

pub(super) fn run(
    handle: &Handle,
    lease: &Lease,
    step: &Step,
    input: &ExecutionEnvelope,
    limit: Duration,
) -> StepResult {
    run_with_probe(
        handle,
        lease,
        step,
        input,
        limit,
        |command, limit, runtime, input| {
            let environment = crate::probe_env::command();
            if matches!(step.kind, StepKind::Command { .. }) {
                probe::run_enveloped(
                    command,
                    Path::new(&lease.job.cwd),
                    limit,
                    runtime,
                    &handle.probe_owner,
                    &environment,
                    input,
                    &lease.cancelled,
                )
            } else {
                probe::run_with_budget(
                    command,
                    Path::new(&lease.job.cwd),
                    limit,
                    runtime,
                    &handle.probe_owner,
                    &environment,
                )
            }
        },
    )
}

fn run_with_probe(
    handle: &Handle,
    lease: &Lease,
    step: &Step,
    input: &ExecutionEnvelope,
    limit: Duration,
    probe: impl FnOnce(
        &str,
        Duration,
        Duration,
        &ExecutionEnvelope,
    ) -> Result<probe::Output, probe::process::ProbeFailure>,
) -> StepResult {
    let previous = input.payload_text();
    if let StepKind::Http { probe } = &step.kind {
        if let Err(reason) = probe.validate() {
            return failed_input_step(&reason);
        }
        let remaining = lease
            .job
            .expires_at
            .map(|at| at.saturating_sub(now(handle)))
            .unwrap_or(probe.timeout_ms);
        if remaining == 0 {
            return failed_input_step("HTTP deadline expired");
        }
        let timeout = Duration::from_millis(remaining.min(probe.timeout_ms)).min(limit);
        let client = match reqwest::blocking::Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
        {
            Ok(client) => client,
            Err(_) => return failed_input_step("HTTP client unavailable"),
        };
        let code = client
            .get(&probe.url)
            .send()
            .ok()
            .map(|response| response.status().as_u16());
        return reading_result(step, probe.reading(code));
    }
    let (command, runtime) = match &step.kind {
        StepKind::Command {
            command,
            timeout_ms,
        } => (command.clone(), Duration::from_millis(*timeout_ms)),
        StepKind::Github { probe } => match probe.command() {
            Ok(command) => (command, Duration::from_millis(probe.timeout_ms)),
            Err(reason) => return failed_input_step(&reason),
        },
        _ => unreachable!(),
    };
    let remaining = lease
        .job
        .expires_at
        .map(|at| Duration::from_millis(at.saturating_sub(now(handle))))
        .unwrap_or(Duration::MAX)
        .min(limit);
    let result = probe(&command, remaining, runtime, input);
    match result {
        Ok(output) => {
            #[cfg(any(test, feature = "test-support"))]
            if !output.success {
                lease.cancelled.record_diagnostic(format!(
                    "command_probe exit={:?} stderr_tail={:?}",
                    output.exit_code,
                    crate::probe::test_receipt::stderr_tail(&output.stderr_text())
                ));
            }
            let output_limit = step
                .execution
                .as_ref()
                .and_then(|contract| contract.limits.as_ref())
                .and_then(|limits| limits.output_bytes)
                .unwrap_or(32768)
                .min(32768);
            if output.stdout.truncated
                || output.stderr.truncated
                || output
                    .stdout
                    .total_bytes
                    .saturating_add(output.stderr.total_bytes)
                    > output_limit
            {
                return failed_input_step("budget_exhausted");
            }
            if output.success {
                if let StepKind::Github { probe } = &step.kind {
                    if probe.resource != GithubResource::PrChecks {
                        let parsed = serde_json::from_str(&output.stdout_text())
                            .map_err(|_| "invalid GitHub JSON".to_string())
                            .and_then(|value| {
                                codeterm_blueprint_types::blueprint_source::github_reading(
                                    probe, value,
                                )
                            });
                        match parsed {
                            Ok(Some(reading)) => return reading_result(step, reading),
                            Ok(None) => {}
                            Err(reason) => return failed_input_step(&reason),
                        }
                    }
                }
            }
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
        Err(failure) => {
            #[cfg(any(test, feature = "test-support"))]
            lease.cancelled.record_diagnostic(format!(
                "command_probe reason={} stderr_tail={:?}",
                failure.primary, failure.stderr_tail
            ));
            command_failure(failure, &previous)
        }
    }
}

fn reading_result(step: &Step, reading: SourceReading) -> StepResult {
    let route = if reading.state == "done"
        || (reading.state == "failing" && !step.on.contains_key("failing"))
    {
        StepOutcome::Done
    } else {
        StepOutcome::Bucket(reading.state.clone())
    };
    StepResult {
        outcome: route,
        stored_outcome: reading.state.into(),
        state: JournalState::Done,
        summary: reading.summary,
        reason: None,
        next_text: reading.output.to_string(),
        decision: None,
        uncertain: false,
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod budget_tests;
