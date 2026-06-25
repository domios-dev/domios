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
        out.push_str(&format!(
            "  - {task}: {kind}{suffix} - {detail}; fix: {fix}\n"
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
