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
                println!("(no tasks)");
            } else {
                print!(
                    "{}",
                    crate::cmd::task_list_render::render_simple_task_list(tasks)
                );
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
    print_hints_stderr(&hints);
    if let Some(nudge) = val.get("_planner_nudge").and_then(|v| v.as_str()) {
        eprintln!("{nudge}");
    }
}

/// With --json, callers pipe stdout into a JSON parser — an error must stay
/// structured (`{"error": ...}`) instead of leaking a human hint that fails jq.
pub(crate) fn print_plan_error(json: bool, message: &str) {
    if json {
        println!(
            "{}",
            serde_json::json!({
                "ok": false,
                "kind": "error",
                "id": null,
                "data": null,
                "hints": [],
                "error": message,
            })
        );
    } else {
        println!("{message}");
    }
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
}
