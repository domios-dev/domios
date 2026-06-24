use serde_json::Value;

pub fn render_relink_report(val: &Value) -> String {
    let mut out = String::new();
    let unlinked = val
        .get("unlinked_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    out.push_str(&format!("Unlinked tasks: {unlinked}\n"));
    for key in ["applied", "suggestions"] {
        let Some(items) = val.get(key).and_then(|v| v.as_array()) else {
            continue;
        };
        if items.is_empty() {
            continue;
        }
        out.push_str(&format!("{key}:\n"));
        for item in items {
            let task = item
                .get("task_name")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let task_id = item.get("task_id").and_then(|v| v.as_str()).unwrap_or("?");
            let section = item
                .get("section_title")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let section_id = item
                .get("section_id")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let reason = item.get("reason").and_then(|v| v.as_str()).unwrap_or("");
            out.push_str(&format!(
                "  - {task} ({task_id}) -> {section} ({section_id}) {reason}\n"
            ));
        }
    }
    out
}
