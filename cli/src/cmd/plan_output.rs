use serde_json::Value;

use crate::cmd::hints::{extract_hints_from_value, print_hints_stderr};
use crate::cmd::print::print_object;

const MUTATION_ID_FIELDS: &[&str] = &[
    "task_id",
    "spec_id",
    "section_id",
    "element_id",
    "milestone_id",
    "stride_id",
    "loop_id",
    "plan_id",
];

pub(crate) fn canonical_mutation_id(val: &Value) -> Option<String> {
    for field in MUTATION_ID_FIELDS {
        if let Some(id) = val
            .get(field)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            return Some(id.to_string());
        }
    }
    None
}

fn canonical_mutation_kind(val: &Value) -> Option<&'static str> {
    for field in MUTATION_ID_FIELDS {
        if val
            .get(field)
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.is_empty())
        {
            return Some(field.trim_end_matches("_id"));
        }
    }
    None
}

fn labeled_mutation_id(val: &Value) -> Option<String> {
    let kind = canonical_mutation_kind(val)?;
    let id = canonical_mutation_id(val)?;
    Some(format!("{kind}_id {id}"))
}

pub(crate) fn planner_json_envelope(val: &Value) -> Option<Value> {
    let id = canonical_mutation_id(val)?;
    let kind = canonical_mutation_kind(val).unwrap_or("mutation");
    let hints = extract_hints_from_value(val);
    let mut envelope = serde_json::json!({
        "ok": true,
        "kind": kind,
        "id": id,
        "data": val,
    });
    if !hints.is_empty() {
        envelope["hints"] = serde_json::json!(hints);
    }
    Some(envelope)
}

fn is_list_response(val: &Value) -> bool {
    val.get("plans").is_some() || val.get("tasks").is_some()
}

fn print_plan_body(val: &Value) {
    if is_list_response(val) {
        if let Some(arr) = val.get("plans").and_then(|v| v.as_array()) {
            if arr.is_empty() {
                println!("(no plans)");
            } else {
                for item in arr {
                    let id = item.get("plan_id").and_then(|v| v.as_str()).unwrap_or("?");
                    let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("?");
                    println!("{id}\t{title}");
                }
            }
            return;
        }
        if let Some(tasks) = val.get("tasks").and_then(|v| v.as_array()) {
            if tasks.is_empty() {
                if let Some(message) = val.get("task_list_message").and_then(Value::as_str) {
                    println!("{message}");
                } else {
                    println!("(no tasks)");
                }
            } else {
                print!(
                    "{}",
                    crate::cmd::task_list_render::render_simple_task_list(tasks)
                );
            }
            if let Some(summary) = val.get("task_list_summary").and_then(Value::as_str) {
                println!("{summary}");
            }
            return;
        }
    }

    if canonical_mutation_id(val).is_some() {
        return;
    }

    if val.get("ok").is_some() {
        println!("OK");
        return;
    }

    print_object(val);
}

pub(crate) fn print_plan_result(json: bool, val: &Value) {
    if json {
        if let Some(envelope) = planner_json_envelope(val) {
            crate::cmd::print::print_json(&envelope);
        } else {
            crate::cmd::print::print_json(val);
        }
        return;
    }

    let hints = extract_hints_from_value(val);
    if let Some(line) = labeled_mutation_id(val) {
        println!("{line}");
    } else {
        print_plan_body(val);
    }
    if let Some(warning) = render_strategy_warning(val) {
        println!("{warning}");
    }
    print_hints_stderr(&hints);
    if let Some(nudge) = val.get("_planner_nudge").and_then(|v| v.as_str()) {
        eprintln!("{nudge}");
    }
}

fn render_strategy_warning(val: &Value) -> Option<String> {
    let warning = val.get("strategy_warning")?;
    let consecutive = warning.get("consecutive_bounces")?.as_u64()?;
    let round = warning.get("review_round")?.as_u64()?;
    let message = warning.get("message")?.as_str()?;
    Some(format!(
        "Strategy warning ({consecutive} consecutive stagnant bounces, review round {round}): {message}"
    ))
}

/// The one error envelope the whole CLI emits under `--json`. `code` carries the
/// JSON-RPC code programmatically so no human-facing line has to leak it.
pub(crate) fn error_envelope(message: &str, code: Option<i32>) -> Value {
    serde_json::json!({
        "ok": false,
        "kind": "error",
        "id": null,
        "data": null,
        "hints": [],
        "error": message,
        "code": code,
    })
}

fn plan_error_line(json: bool, message: &str) -> String {
    if json {
        error_envelope(message, None).to_string()
    } else {
        message.to_string()
    }
}

/// With --json, callers pipe stdout into a JSON parser — an error must stay
/// structured (`{"error": ...}`) instead of leaking a human hint that fails jq.
pub(crate) fn print_plan_error(json: bool, message: &str) {
    println!("{}", plan_error_line(json, message));
}

pub(crate) fn print_mutation_tail(json: bool, val: &Value) {
    if json {
        if let Some(envelope) = planner_json_envelope(val) {
            crate::cmd::print::print_json(&envelope);
        } else {
            crate::cmd::print::print_json(val);
        }
        return;
    }
    let hints = extract_hints_from_value(val);
    if let Some(line) = labeled_mutation_id(val) {
        println!("{line}");
    } else if val.get("ok").is_some() {
        println!("OK");
    }
    print_hints_stderr(&hints);
}

#[cfg(test)]
mod tests {
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
        assert_eq!(envelope["ok"], true);
        assert_eq!(envelope["kind"], "task");
        assert_eq!(envelope["id"], "task_abc123");
        assert_eq!(envelope["data"]["task_id"], "task_abc123");
        assert_eq!(envelope["hints"][0], "hint: fill sections");
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
                "message": "Stop iterating this strategy. Re-diagnose per `codeterm docs BOUNCE`."
            }
        });
        let rendered = render_strategy_warning(&val).unwrap();
        assert!(rendered.contains("3 consecutive stagnant bounces"));
        assert!(rendered.contains("review round 4"));
        assert!(rendered.contains("codeterm docs BOUNCE"));
    }

    const PLAN_NOT_FOUND: &str = "✗ plan 'zzzzzzzz' not found.\nfix: codeterm plan list --active";

    #[test]
    fn plan_json_error_line_is_the_shared_envelope_with_a_null_code() {
        let parsed: Value =
            serde_json::from_str(&plan_error_line(true, PLAN_NOT_FOUND)).expect("one JSON envelope");

        assert_eq!(parsed["ok"], Value::Bool(false));
        assert_eq!(parsed["kind"], "error");
        assert_eq!(parsed["code"], Value::Null);
        assert_eq!(parsed["error"], PLAN_NOT_FOUND);
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
}
