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
    let timeout = Duration::from_millis(code.limits.wall_ms.unwrap_or(3000)).min(limit);
    match crate::code_runtime::run_from_directory(
        &code,
        Path::new(&lease.job.cwd),
        &input,
        timeout,
        &handle.probe_owner,
        &lease.cancelled,
    ) {
        Ok(output) if !output.success => failed_input_step(if code.limits.mem_mb.is_some() {
            "budget_exhausted"
        } else {
            "sandbox_denied"
        }),
        Ok(output) => {
            let bound = code.limits.output_bytes.unwrap_or(32768).min(32768);
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
                summary: "confined code completed".into(),
                reason: None,
                next_text: String::from_utf8(envelope.encode().unwrap_or_default())
                    .unwrap_or_default(),
                decision: None,
                uncertain: false,
            }
        }
        Err(failure) => {
            let mut result = failed_input_step(failure.reason);
            result.uncertain = failure.uncertain;
            result
        }
    }
}

fn minimum(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (left, right) => left.or(right),
    }
}
