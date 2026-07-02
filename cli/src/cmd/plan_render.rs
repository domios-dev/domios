use serde_json::Value;

use crate::cmd::hints::print_hints;
use crate::cmd::print::print_object;

pub(crate) fn render_plan_sections(plan: &Value) -> String {
    fn walk(sections: &[Value], depth: usize, out: &mut String) {
        for section in sections {
            let id = section.get("id").and_then(|v| v.as_str()).unwrap_or("?");
            let title = section.get("title").and_then(|v| v.as_str()).unwrap_or("?");
            out.push_str(&format!("{}{}\t{}\n", "  ".repeat(depth), id, title));
            if let Some(children) = section.get("subsections").and_then(|v| v.as_array()) {
                walk(children, depth + 1, out);
            }
        }
    }

    let mut out = String::new();
    if let Some(sections) = plan.get("sections").and_then(|v| v.as_array()) {
        walk(sections, 0, &mut out);
    }
    if out.is_empty() {
        out.push_str("(no sections)\n");
    }
    out
}

pub(crate) fn render_coverage(val: &Value) -> String {
    let mut out = String::new();
    let frac = val
        .get("plan_covered_fraction")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    out.push_str(&format!("Coverage: {:.0}%\n", frac * 100.0));
    if let Some(sections) = val.get("sections").and_then(|v| v.as_array()) {
        for s in sections {
            let title = s.get("title").and_then(|v| v.as_str()).unwrap_or("?");
            let total = s.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
            let status = s.get("status").and_then(|v| v.as_str()).unwrap_or("none");
            out.push_str(&format!("  - {title}: {total} task(s) [{status}]\n"));
        }
    }
    let gaps: Vec<&str> = val
        .get("gaps")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    if gaps.is_empty() {
        out.push_str("Gaps: none — every section has at least one task.\n");
    } else {
        out.push_str(&format!("Gaps ({}): {}\n", gaps.len(), gaps.join(", ")));
    }
    let unlinked_count = val
        .get("unlinked_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    if unlinked_count > 0 {
        out.push_str(&format!("Unlinked tasks: {unlinked_count}\n"));
    }
    out
}

pub(crate) fn print_plan_result(json: bool, val: &Value) {
    if json {
        println!("{}", serde_json::to_string_pretty(val).unwrap());
        return;
    }

    let hints: Vec<String> = val
        .get("_hints")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    if val.get("plan_id").is_some() && val.get("plan").is_none() {
        if let Some(id) = val.get("plan_id").and_then(|v| v.as_str()) {
            println!("{}", id);
        } else {
            print_object(val);
        }
    } else if val.get("task_id").is_some()
        || val.get("section_id").is_some()
        || val.get("loop_id").is_some()
        || val.get("milestone_id").is_some()
        || val.get("element_id").is_some()
    {
        let id = val
            .get("task_id")
            .or_else(|| val.get("section_id"))
            .or_else(|| val.get("loop_id"))
            .or_else(|| val.get("milestone_id"))
            .or_else(|| val.get("element_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        println!("{}", id);
    } else if val.get("plans").is_some() {
        if let Some(arr) = val.get("plans").and_then(|v| v.as_array()) {
            if arr.is_empty() {
                println!("(no plans)");
            } else {
                for item in arr {
                    let id = item.get("plan_id").and_then(|v| v.as_str()).unwrap_or("?");
                    let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("?");
                    println!("{}\t{}", id, title);
                }
            }
        }
    } else if let Some(tasks) = val.get("tasks").and_then(|v| v.as_array()) {
        if tasks.is_empty() {
            println!("(no tasks)");
        } else {
            print!(
                "{}",
                crate::cmd::task_list_render::render_simple_task_list(tasks)
            );
        }
    } else if val.get("ok").is_some() {
        println!("OK");
    } else {
        print_object(val);
    }
    print_hints(&hints);
    if let Some(nudge) = val.get("_planner_nudge").and_then(|v| v.as_str()) {
        eprintln!("{nudge}");
    }
}
