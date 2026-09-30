use super::*;

#[test]
fn approval_accepts_drafted_execution_tasks_while_preserving_other_errors() {
    let report = json!({
        "error_count": 2,
        "warning_count": 1,
        "ok": false,
        "findings": [
            {"severity": "warning", "kind": "tasks_before_approval"},
            {"severity": "error", "kind": "missing_goal"}
        ]
    });
    assert_eq!(report["findings"].as_array().unwrap().len(), 2);
    assert_eq!(report["findings"][0]["severity"], "warning");
    assert_eq!(report["findings"][1]["kind"], "missing_goal");
}
