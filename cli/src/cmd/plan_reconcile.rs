use serde_json::Value;

pub fn render_reconcile(val: &Value) -> String {
    let mut out = String::new();
    let drift = val
        .get("drift")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    out.push_str(&format!("Drift items: {}\n", drift.len()));
    if let Some(counts) = val.get("counts").and_then(|v| v.as_object()) {
        let mut keys = counts.keys().cloned().collect::<Vec<_>>();
        keys.sort();
        for key in keys {
            let count = counts.get(&key).and_then(|v| v.as_u64()).unwrap_or(0);
            out.push_str(&format!("  {key}: {count}\n"));
        }
    }
    for item in drift {
        let task = item.get("task_id").and_then(|v| v.as_str()).unwrap_or("?");
        let kind = item.get("kind").and_then(|v| v.as_str()).unwrap_or("?");
        let detail = item.get("detail").and_then(|v| v.as_str()).unwrap_or("");
        let fix = item
            .get("suggested_fix")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let not_mine = item
            .get("not_mine")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let suffix = if not_mine { " [not-mine]" } else { "" };
        let command = reconcile_command(
            item.get("plan_id").and_then(|v| v.as_str()).unwrap_or("?"),
            task,
            item.get("kind").and_then(|v| v.as_str()).unwrap_or(""),
        );
        out.push_str(&format!(
            "  - {task}: {kind}{suffix} - {detail}; fix: {fix}; command: {command}\n"
        ));
    }
    let fixed = val
        .get("fixed")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if !fixed.is_empty() {
        out.push_str(&format!("Applied fixes: {}\n", fixed.len()));
        for item in fixed {
            let task = item.get("task_id").and_then(|v| v.as_str()).unwrap_or("?");
            let action = item.get("action").and_then(|v| v.as_str()).unwrap_or("?");
            out.push_str(&format!("  - {task}: {action}\n"));
        }
    }
    if val
        .get("accepted_signal")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        == "none"
    {
        out.push_str("Accepted-but-not-done: skipped; no unambiguous acceptance signal exists in the task model.\n");
    }
    out
}

pub fn annotate_reconcile_json(val: &mut Value) {
    let Some(items) = val.get_mut("drift").and_then(Value::as_array_mut) else {
        return;
    };
    for item in items {
        let Some(object) = item.as_object_mut() else {
            continue;
        };
        let plan_id = object.get("plan_id").and_then(Value::as_str).unwrap_or("?");
        let task_id = object.get("task_id").and_then(Value::as_str).unwrap_or("?");
        let kind = object.get("kind").and_then(Value::as_str).unwrap_or("");
        object.insert(
            "suggested_guidance".into(),
            serde_json::json!({
                "kind": "executable_command",
                "command": reconcile_command(plan_id, task_id, kind),
                "fix_label": object.get("suggested_fix").cloned().unwrap_or(Value::Null)
            }),
        );
    }
}

fn reconcile_command(plan_id: &str, task_id: &str, kind: &str) -> String {
    match kind {
        "stale_in_review" | "accepted_but_not_done" => {
            format!("codeterm task show {task_id} && codeterm plan task close {plan_id} {task_id}")
        }
        "in_progress_tab_gone" | "bound_to_closed_tab" => {
            format!("codeterm task show {task_id} && codeterm plan task update {plan_id} {task_id} --status blocked --status-label needs-reassign")
        }
        _ => format!("codeterm task show {task_id} && codeterm plan reconcile {plan_id} --fix"),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{annotate_reconcile_json, render_reconcile};

    #[test]
    fn render_reconcile_includes_exact_plan_and_task_commands() {
        let rendered = render_reconcile(&json!({
            "drift": [
                {
                    "plan_id": "pl_1",
                    "task_id": "task_a",
                    "kind": "stale_in_review",
                    "detail": "stale",
                    "suggested_fix": "review"
                },
                {
                    "plan_id": "pl_1",
                    "task_id": "task_b",
                    "kind": "bound_to_closed_tab",
                    "detail": "closed",
                    "suggested_fix": "clear"
                }
            ]
        }));

        assert!(
            rendered.contains("codeterm task show task_a && codeterm plan task close pl_1 task_a")
        );
        assert!(rendered.contains(
            "codeterm task show task_b && codeterm plan task update pl_1 task_b --status blocked --status-label needs-reassign"
        ));
    }

    #[test]
    fn failclosed_exit_reconcile_json_labels_command_guidance() {
        let mut value = json!({
            "drift": [{
                "plan_id": "pl_1",
                "task_id": "task_a",
                "kind": "stale_in_review",
                "suggested_fix": "stale-review"
            }]
        });
        annotate_reconcile_json(&mut value);
        let guidance = &value["drift"][0]["suggested_guidance"];
        assert_eq!(guidance["kind"], "executable_command");
        assert_eq!(guidance["fix_label"], "stale-review");
        assert_eq!(
            guidance["command"],
            "codeterm task show task_a && codeterm plan task close pl_1 task_a"
        );
        assert!(!guidance["command"].as_str().unwrap().contains("stale-review"));
    }
}
