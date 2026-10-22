use super::*;
use codeterm_blueprint_types::blueprint_pipeline::MAX_RESULT_SUMMARY_CHARS;

#[test]
fn blueprint_v2_runtime_failed_stderr_tail_survives_journal_bound() {
    let body_sha256 = "a".repeat(64);
    let stderr = format!(
        "Traceback:\n{}\nValueError: bad row\napi_key=opaque-private\n",
        "x".repeat(6000)
    );
    let summary = failure_summary(&body_sha256, &stderr);
    let stored = StepJournalEntry::bound_summary(&summary);
    assert_eq!(stored, summary);
    assert_eq!(stored.chars().count(), MAX_RESULT_SUMMARY_CHARS);
    assert!(stored.ends_with("ValueError: bad row [redacted]"));
    assert!(stored.contains(&format!("body_sha256={body_sha256}")));
    assert!(!stored.contains("opaque-private"));
    assert!(!stored.chars().any(char::is_control));
}

fn captured(stdout: &[u8], stderr: &[u8], success: bool) -> probe::Output {
    let mut bytes = if success { 0_i32 } else { 1_i32 }.to_le_bytes().to_vec();
    bytes.extend_from_slice(&(stdout.len() as u64).to_le_bytes());
    bytes.extend_from_slice(stdout);
    bytes.extend_from_slice(stderr);
    probe::Output {
        bytes,
        success,
        exit_code: Some(if success { 0 } else { 1 }),
        stdout: crate::command::Capture {
            total_bytes: stdout.len() as u64,
            retained_bytes: stdout.len() as u64,
            truncated: false,
        },
        stderr: crate::command::Capture {
            total_bytes: stderr.len() as u64,
            retained_bytes: stderr.len() as u64,
            truncated: false,
        },
        ..Default::default()
    }
}

#[test]
fn blueprint_v2_runtime_code_exit_routes_failed_with_bounded_stderr() {
    use codeterm_blueprint_types::blueprint_document::Document;
    use serde_json::json;
    let doc = Document::from_definition(&json!({
        "schema_version":2,"kind":"pipeline","name":"portable-failure",
        "trigger":{"type":"input","kind":{"lit":"text"},"source":{"lit":{"text":"opaque"}}},
        "steps":[{"id":"body","code":{"lang":"python","src":"def main(value): raise ValueError('bad row')"},"routes":{"failed":"@next"}},
                 {"id":"recover","run":{"lit":"echo recovered"}}],
        "result":{"to":"notify"}
    })).unwrap();
    let lowered = doc.lower().unwrap();
    let spec: BlueprintSpec = serde_json::from_value(lowered["spec"].clone()).unwrap();
    let stderr = format!(
        "{}\nValueError: bad row\napi_key=opaque-private\n",
        "x".repeat(6000)
    );
    let failed = completed(
        captured(b"", stderr.as_bytes(), false),
        32768,
        &"a".repeat(64),
    );
    assert_eq!(failed.outcome, StepOutcome::Failed);
    assert_eq!(failed.state, JournalState::Failed);
    assert_eq!(failed.stored_outcome.as_str(), "failed");
    assert_eq!(failed.reason.as_ref().unwrap().as_str(), "code_failed");
    assert_eq!(
        StepJournalEntry::bound_summary(&failed.summary),
        failed.summary
    );
    assert!(failed.summary.contains("ValueError: bad row [redacted]"));
    assert!(!failed.summary.contains("opaque-private"));
    assert!(!failed.uncertain);
    let running = RunnerState::start(&spec).unwrap();
    let recovered = blueprint_pipeline::advance(&running, failed.outcome).unwrap();
    assert_eq!(recovered.current_step, Some(1));
    assert_eq!(recovered.status, RunStatus::Running);
    let done = blueprint_pipeline::advance(&recovered, StepOutcome::Done).unwrap();
    assert_eq!(done.status, RunStatus::Done);
    let mut unhandled = spec;
    unhandled.steps[0].on.clear();
    let stopped = blueprint_pipeline::advance(
        &RunnerState::start(&unhandled).unwrap(),
        StepOutcome::Failed,
    )
    .unwrap();
    assert_eq!(stopped.status, RunStatus::Failed);
}

#[test]
fn blueprint_v2_runtime_code_capture_budgets_and_contract_are_portable() {
    let encoded = ExecutionEnvelope::from_text(&"x".repeat(20 * 1024))
        .encode()
        .unwrap();
    let valid = completed(captured(&encoded, b"", true), 32768, "opaque-hash");
    assert_eq!(valid.state, JournalState::Done);
    let envelope = ExecutionEnvelope::decode(valid.next_text.as_bytes()).unwrap();
    assert_eq!(envelope.payload_text(), "x".repeat(20 * 1024));
    let exceeded = completed(
        captured(&encoded, b"diagnostic", true),
        encoded.len() as u64,
        "opaque-hash",
    );
    assert_eq!(exceeded.reason.unwrap(), JournalReason::BudgetExhausted);
    let mut truncated = captured(&encoded, b"", true);
    truncated.stdout.truncated = true;
    truncated.stdout.total_bytes += 1;
    assert_eq!(
        completed(truncated, 32768, "opaque-hash").reason.unwrap(),
        JournalReason::BudgetExhausted
    );
    let malformed = completed(captured(b"{}", b"", true), 32768, "opaque-hash");
    assert_eq!(malformed.reason.unwrap(), JournalReason::ContractViolation);
}
