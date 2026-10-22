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
