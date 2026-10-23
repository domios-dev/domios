use super::*;
use codeterm_blueprint_types::blueprint_document::{Code, Limits};

pub(super) fn run(
    handle: &Handle,
    lease: &Lease,
    step: &Step,
    code: &Code,
    input: &ExecutionEnvelope,
    limit: Duration,
) -> StepResult {
    let input = match input.encode() {
        Ok(input) => input,
        Err(_) => return failed_input_step("contract_violation"),
    };
    let mut code = code.clone();
    if let Some(limits) = step
        .execution
        .as_ref()
        .and_then(|contract| contract.limits.as_ref())
    {
        code.limits = Limits {
            wall_ms: minimum(code.limits.wall_ms, limits.wall_ms),
            mem_mb: minimum(code.limits.mem_mb, limits.mem_mb),
            output_bytes: minimum(code.limits.output_bytes, limits.output_bytes),
        };
    }
    let body_sha256 =
        codeterm_blueprint_types::blueprint_document::code_contract::body_sha256(&code)
            .unwrap_or_default();
    let timeout = Duration::from_millis(code.limits.wall_ms.unwrap_or(3000));
    match crate::code_runtime::run_with_output(
        &code,
        &input,
        limit,
        timeout,
        &handle.probe_owner,
        &lease.cancelled,
        step.execution
            .as_ref()
            .and_then(|contract| contract.output.as_ref()),
    ) {
        Ok(output) => completed(
            output,
            code.limits.output_bytes.unwrap_or(32768).min(32768),
            &body_sha256,
        ),
        Err(failure) => {
            #[cfg(any(test, feature = "test-support"))]
            lease.cancelled.record_diagnostic(format!(
                "code_runtime reason={} stderr_tail={:?}",
                failure.reason, failure.stderr_tail
            ));
            let mut result = failed_input_step(failure.reason);
            result.summary = format!(
                "code stopped: {}; body_sha256={body_sha256}",
                failure.reason
            );
            result.uncertain = failure.uncertain;
            result
        }
    }
}

fn completed(output: probe::Output, bound: u64, body_sha256: &str) -> StepResult {
    if !output.success {
        let mut result = failed_input_step("code_failed");
        result.summary = failure_summary(body_sha256, &output.stderr_text());
        return result;
    }
    if output.stdout.truncated
        || output.stderr.truncated
        || output
            .stdout
            .total_bytes
            .saturating_add(output.stderr.total_bytes)
            > bound
    {
        return failed_input_step("budget_exhausted");
    }
    let envelope = match ExecutionEnvelope::decode(output.stdout_text().as_bytes()) {
        Ok(envelope) => envelope,
        Err(_) => return failed_input_step("contract_violation"),
    };
    StepResult {
        outcome: StepOutcome::Done,
        stored_outcome: JournalOutcome::Done,
        state: JournalState::Done,
        summary: format!("code completed; body_sha256={body_sha256}"),
        reason: None,
        next_text: String::from_utf8(envelope.encode().unwrap_or_default()).unwrap_or_default(),
        decision: None,
        uncertain: false,
    }
}

fn minimum(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (left, right) => left.or(right),
    }
}

fn failure_summary(body_sha256: &str, stderr: &str) -> String {
    use codeterm_blueprint_types::blueprint_pipeline::MAX_RESULT_SUMMARY_CHARS;
    let prefix = format!("code failed; body_sha256={body_sha256}: ");
    let redacted = codeterm_util::redact::text(stderr);
    let flattened = redacted.split_whitespace().collect::<Vec<_>>().join(" ");
    let remaining = MAX_RESULT_SUMMARY_CHARS.saturating_sub(prefix.chars().count());
    let reversed: String = flattened
        .chars()
        .filter(|c| !c.is_control())
        .rev()
        .take(remaining)
        .collect();
    let tail: String = reversed.chars().rev().collect();
    prefix + &tail
}

#[cfg(test)]
mod tests;
