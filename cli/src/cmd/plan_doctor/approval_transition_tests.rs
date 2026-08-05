use super::*;

#[test]
fn approval_accepts_drafted_execution_tasks_while_preserving_other_errors() {
    let mut report = json!({
        "error_count": 2,
        "warning_count": 0,
        "ok": false,
        "findings": [
            {"severity": "error", "kind": "tasks_before_approval"},
            {"severity": "error", "kind": "missing_goal"}
        ]
    });
    remove_approval_transition_findings(&mut report);
    assert_eq!(report["error_count"], 1);
    assert_eq!(report["ok"], false);
    assert_eq!(report["findings"].as_array().unwrap().len(), 1);
    assert_eq!(report["findings"][0]["kind"], "missing_goal");
}
