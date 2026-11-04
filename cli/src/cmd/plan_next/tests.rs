use super::*;
use serde_json::json;

#[test]
fn an_empty_detail_page_never_claims_the_plan_has_no_actions() {
    let value = json!({"next_actions":[],"details":{"totals":{"next_actions":1162},"windows":{"next_actions":{"hint":"1162 total; use --page 1 or --all"}}}});
    let output = render_next(&value);
    assert!(output.contains("No next actions on this page."));
    assert!(output.contains("--page 1 or --all"));
    assert!(!output.contains("nothing pending"));
}

#[test]
fn renders_numbered_priority_list() {
    let val = json!({
        "plan_id": "pl_1",
        "next_actions": [
            {"priority": 1, "kind": "gate", "task_id": "t_r", "message": "gate t_r (rev)"},
            {"priority": 5, "kind": "dispatch", "task_id": "t_p", "message": "dispatch t_p (pend)"}
        ]
    });
    let out = render_next(&val);
    assert!(out.contains("Next actions (2):"));
    assert!(out.contains("1. [gate] gate t_r (rev)"));
    assert!(out.contains("2. [dispatch] dispatch t_p (pend)"));
}
