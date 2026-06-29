use serde_json::Value;

pub fn render_next(val: &Value) -> String {
    let mut out = String::new();
    let actions = val
        .get("next_actions")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if actions.is_empty() {
        out.push_str("No next actions — nothing pending, blocked, drifting, or awaiting a gate.\n");
        return out;
    }
    out.push_str(&format!("Next actions ({}):\n", actions.len()));
    for (i, a) in actions.iter().enumerate() {
        let kind = a.get("kind").and_then(|v| v.as_str()).unwrap_or("?");
        let msg = a.get("message").and_then(|v| v.as_str()).unwrap_or("");
        out.push_str(&format!("  {}. [{kind}] {msg}\n", i + 1));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn renders_numbered_priority_list() {
        let val = json!({
            "plan_id": "pl_1",
            "next_actions": [
                {"priority": 1, "kind": "gate", "task_id": "t_r", "message": "gate t_r (rev)"},
                {"priority": 5, "kind": "dispatch", "task_id": "t_p", "message": "dispatch t_p (pend)"}
            ]
        });
        let out = render_next(&val);
        assert!(out.contains("Next actions (2):"));
        assert!(out.contains("1. [gate] gate t_r (rev)"));
        assert!(out.contains("2. [dispatch] dispatch t_p (pend)"));
    }

    #[test]
    fn renders_empty_state() {
        let out = render_next(&json!({ "plan_id": "pl_1", "next_actions": [] }));
        assert!(out.contains("No next actions"));
    }
}
