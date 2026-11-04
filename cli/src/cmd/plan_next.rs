use serde_json::Value;

pub fn render_next(val: &Value) -> String {
    let mut out = String::new();
    let actions = val
        .get("next_actions")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let total = val
        .pointer("/details/totals/next_actions")
        .and_then(Value::as_u64)
        .unwrap_or(actions.len() as u64);
    if actions.is_empty() {
        out.push_str(if total == 0 {
            "No next actions — nothing pending, blocked, drifting, or awaiting a gate.\n"
        } else {
            "No next actions on this page.\n"
        });
        out.push_str(&crate::cmd::plan_state::render_detail_windows(val));
        return out;
    }
    out.push_str(&format!("Next actions ({total}):\n"));
    for (i, a) in actions.iter().enumerate() {
        let kind = a.get("kind").and_then(|v| v.as_str()).unwrap_or("?");
        let msg = a.get("message").and_then(|v| v.as_str()).unwrap_or("");
        out.push_str(&format!("  {}. [{kind}] {msg}\n", i + 1));
    }
    out.push_str(&crate::cmd::plan_state::render_detail_windows(val));
    out
}

#[cfg(test)]
mod tests;
