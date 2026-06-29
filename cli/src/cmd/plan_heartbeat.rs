use serde_json::Value;

pub(crate) fn render_heartbeat_block(heartbeat: &Value, out: &mut String) {
    let focus = heartbeat
        .get("current_focus")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty());
    let lanes = non_empty_strings(heartbeat, "active_lanes");
    let rules = non_empty_strings(heartbeat, "rules");
    let learnings = heartbeat
        .get("learnings")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("text").and_then(|v| v.as_str()))
                .filter(|text| !text.trim().is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if focus.is_none() && lanes.is_empty() && rules.is_empty() && learnings.is_empty() {
        return;
    }
    out.push_str("## Orchestrator Heartbeat\n\n");
    if let Some(focus) = focus {
        out.push_str(&format!("**Current focus:** {}\n\n", focus.trim()));
    }
    render_named_strings("Active lanes", &lanes, out);
    render_named_strings("Rules", &rules, out);
    if !learnings.is_empty() {
        out.push_str("### Learnings\n\n");
        for learning in learnings.iter().rev().take(12) {
            out.push_str(&format!("- {}\n", learning.trim()));
        }
        out.push('\n');
    }
}

fn non_empty_strings<'a>(value: &'a Value, key: &str) -> Vec<&'a str> {
    value
        .get(key)
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str())
                .filter(|text| !text.trim().is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn render_named_strings(label: &str, items: &[&str], out: &mut String) {
    if items.is_empty() {
        return;
    }
    out.push_str(&format!("### {label}\n\n"));
    for item in items {
        out.push_str(&format!("- {}\n", item.trim()));
    }
    out.push('\n');
}
