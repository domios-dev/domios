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

#[cfg(test)]
pub(crate) fn planner_json_envelope(val: &Value) -> Option<Value> {
    canonical_mutation_id(val)?;
    Some(crate::cmd::output::json_envelope(val))
}

fn is_list_response(val: &Value) -> bool {
    val.get("plans").is_some() || val.get("tasks").is_some()
}

fn plan_list_line(item: &Value) -> String {
    let id = item.get("plan_id").and_then(|v| v.as_str()).unwrap_or("?");
    let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("?");
    match item.get("orphaned").and_then(Value::as_str) {
        Some(reason) => format!("{id}\t{title}\t[flagged: {reason}]"),
        None => format!("{id}\t{title}"),
    }
}

fn print_plan_body(val: &Value) {
    if is_list_response(val) && !crate::cmd::output::verbose() {
        print!("{}", crate::cmd::output::compact_list(val, None));
        return;
    }
    if is_list_response(val) {
        if let Some(arr) = val.get("plans").and_then(|v| v.as_array()) {
            if arr.is_empty() {
                println!("(no plans)");
            } else {
                for item in arr {
                    println!("{}", plan_list_line(item));
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
    crate::cmd::print::exit_if_cli_refusal(json, val);
    if json {
        crate::cmd::print::print_json(val);
        return;
    }

    let hints = extract_hints_from_value(val);
    if is_list_response(val) && !crate::cmd::output::verbose() {
        print_plan_body(val);
        return;
    }
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

/// The one error envelope the whole CLI emits under `--json`, built by the
/// shared error contract.
pub(crate) fn error_envelope(message: &str, code: Option<i32>) -> Value {
    error_envelope_with_data(message, code, None)
}

pub(crate) fn error_envelope_with_data(
    message: &str,
    code: Option<i32>,
    data: Option<&Value>,
) -> Value {
    crate::cmd::error_contract::error_envelope(message, code, data)
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
    crate::cmd::print::exit_if_cli_refusal(json, val);
    if json {
        crate::cmd::print::print_json(val);
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
mod tests;
