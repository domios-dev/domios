use super::*;
use serde_json::json;

fn stdout_lines(val: &Value) -> Vec<String> {
    let mut buf = Vec::new();
    {
        let mut out = std::io::Cursor::new(&mut buf);
        let hints = extract_hints_from_value(val);
        print_plan_body(val);
        print_hints_stderr_to(&hints, &mut std::io::stderr()).ok();
        if let Some(line) = labeled_mutation_id(val) {
            use std::io::Write;
            let _ = writeln!(out, "{line}");
        }
    }
    String::from_utf8(buf)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}

fn print_hints_stderr_to(hints: &[String], w: &mut impl std::io::Write) -> std::io::Result<()> {
    for hint in hints {
        writeln!(w, "{hint}")?;
    }
    Ok(())
}

#[test]
fn canonical_id_priority_prefers_task_over_plan() {
    let val = json!({ "plan_id": "pl_1", "task_id": "task_1" });
    assert_eq!(canonical_mutation_id(&val).as_deref(), Some("task_1"));
}

#[test]
fn mutation_stdout_ends_with_canonical_id() {
    let val = json!({
        "plan_id": "pl_1",
        "task_id": "task_abc123",
        "_hints": ["hint: fill sections"]
    });
    let lines = stdout_lines(&val);
    assert_eq!(
        lines.last().map(String::as_str),
        Some("task_id task_abc123")
    );
    assert!(!lines.iter().any(|l| l.starts_with("hint:")));
}

#[test]
fn plan_create_human_output_is_labeled_id_only_on_stdout() {
    let val = json!({
        "plan_id": "pl_new",
        "plan": { "id": "pl_new", "title": "T" },
        "_hints": ["hint: add sections"]
    });
    let lines = stdout_lines(&val);
    assert_eq!(lines, vec!["plan_id pl_new"]);
}

#[test]
fn mutation_json_uses_planner_envelope() {
    let val = json!({
        "plan_id": "pl_1",
        "task_id": "task_abc123",
        "_hints": ["hint: fill sections"]
    });
    let envelope = planner_json_envelope(&val).unwrap();
    assert_eq!(envelope["result"]["id"], "task_abc123");
    assert_eq!(envelope["result"]["task_id"], "task_abc123");
    assert_eq!(envelope["result"]["_hints"][0], "hint: fill sections");
}

#[test]
fn spec_id_is_canonical_mutation_id() {
    let val = json!({ "spec_id": "sp_1" });
    assert_eq!(canonical_mutation_id(&val).as_deref(), Some("sp_1"));
    assert_eq!(labeled_mutation_id(&val).as_deref(), Some("spec_id sp_1"));
}

#[test]
fn list_responses_have_no_canonical_mutation_id() {
    let val = json!({ "plans": [{ "plan_id": "pl_1", "title": "One" }] });
    assert!(canonical_mutation_id(&val).is_none());
}

#[test]
fn strategy_warning_plain_text_names_count_round_and_recovery() {
    let val = json!({
        "task_id": "task_1",
        "strategy_warning": {
            "consecutive_bounces": 3,
            "review_round": 4,
            "message": "Stop iterating this strategy. Re-diagnose per `domios docs BOUNCE`."
        }
    });
    let rendered = render_strategy_warning(&val).unwrap();
    assert!(rendered.contains("3 consecutive stagnant bounces"));
    assert!(rendered.contains("review round 4"));
    assert!(rendered.contains("domios docs BOUNCE"));
}

const PLAN_NOT_FOUND: &str = "✗ plan 'zzzzzzzz' not found.\nfix: domios plan list --active";

#[test]
fn plan_json_error_line_is_the_shared_envelope_with_a_null_rpc_code() {
    let parsed: Value = serde_json::from_str(&plan_error_line(true, PLAN_NOT_FOUND))
        .expect("one JSON envelope");

    assert_eq!(parsed["ok"], Value::Bool(false));
    assert_eq!(parsed["kind"], "error");
    assert_eq!(parsed["error"]["rpc_code"], Value::Null);
    assert_eq!(parsed["error"]["code"], "not_found");
    assert_eq!(parsed["error"]["message"], PLAN_NOT_FOUND);
}

#[test]
fn plan_human_error_line_keeps_its_fix_hint_verbatim() {
    assert_eq!(plan_error_line(false, PLAN_NOT_FOUND), PLAN_NOT_FOUND);
}

#[test]
fn plan_and_cli_errors_share_one_envelope_shape() {
    let plan_keys = envelope_keys(&serde_json::from_str(&plan_error_line(true, "x")).unwrap());
    let cli_keys = envelope_keys(&error_envelope("x", Some(-32603)));

    assert_eq!(plan_keys, cli_keys);
}

fn envelope_keys(value: &Value) -> Vec<String> {
    let mut keys: Vec<String> = value
        .as_object()
        .expect("envelope is an object")
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

#[test]
fn absent_strategy_warning_stays_silent() {
    assert_eq!(
        render_strategy_warning(&json!({ "task_id": "task_1" })),
        None
    );
}

#[test]
fn plan_list_line_carries_the_orphan_flag() {
    let flagged = plan_list_line(&json!({
        "plan_id": "pl_1",
        "title": "Plan",
        "orphaned": "reason-token",
    }));
    assert!(flagged.starts_with("pl_1\tPlan\t"));
    assert!(flagged.contains("reason-token"));
    assert_eq!(
        plan_list_line(&json!({ "plan_id": "pl_2", "title": "Plan" })),
        "pl_2\tPlan"
    );
}
