use serde_json::Value;

pub(crate) fn render_plan_markdown(plan: &Value) -> String {
    let mut out = String::new();
    let title = plan.get("title").and_then(|v| v.as_str()).unwrap_or("?");
    let id = plan.get("id").and_then(|v| v.as_str()).unwrap_or("?");
    let status = plan.get("status").and_then(|v| v.as_str()).unwrap_or("?");
    out.push_str(&format!("# {title} (`{id}`)\n\n"));
    out.push_str(&format!("**Status:** {status}\n\n"));
    if let Some(goal) = plan.get("goal").filter(|v| !v.is_null()) {
        render_goal_block(goal, &mut out);
    }
    if let Some(heartbeat) = plan.get("heartbeat") {
        crate::cmd::plan_heartbeat::render_heartbeat_block(heartbeat, &mut out);
    }
    if let Some(count) = plan
        .get("_linking")
        .and_then(|v| v.get("unlinked_count"))
        .and_then(|v| v.as_u64())
        .filter(|count| *count > 0)
    {
        let id = plan.get("id").and_then(|v| v.as_str()).unwrap_or("?");
        out.push_str(&format!(
            "**Unlinked tasks:** {count} (preview: `codeterm plan task relink {id}`; apply: `codeterm plan task relink {id} --apply`)\n\n"
        ));
    }
    if let Some(sections) = plan.get("sections").and_then(|v| v.as_array()) {
        if !sections.is_empty() {
            out.push_str("## Sections\n\n");
            for s in sections {
                let st = s.get("title").and_then(|v| v.as_str()).unwrap_or("?");
                out.push_str(&format!("### {st}\n\n"));
                if let Some(body) = s.get("body").and_then(|v| v.as_str()) {
                    if !body.is_empty() {
                        out.push_str(body);
                        out.push_str("\n\n");
                    }
                }
            }
        }
    }
    if let Some(tasks) = plan.get("tasks").and_then(|v| v.as_array()) {
        if !tasks.is_empty() {
            out.push_str("## Tasks\n\n");
            for t in tasks {
                let name = t.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                let tid = t.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                let st = t.get("status").and_then(|v| v.as_str()).unwrap_or("?");
                out.push_str(&format!("- **{name}** (`{tid}`) — {st}\n"));
                render_task_quality(t, &mut out);
            }
            out.push('\n');
        }
    }
    out
}

fn render_task_quality(task: &Value, out: &mut String) {
    if let Some(items) = task.get("acceptance_criteria").and_then(|v| v.as_array()) {
        for item in items.iter().filter_map(|v| v.as_str()) {
            out.push_str(&format!("  - criterion: {item}\n"));
        }
    }
    if let Some(test_plan) = task.get("test_plan").and_then(|v| v.as_str()) {
        if !test_plan.is_empty() {
            out.push_str(&format!("  - test plan: {test_plan}\n"));
        }
    }
    if let Some(correctness) = task.get("correctness").and_then(|v| v.as_str()) {
        if !correctness.is_empty() {
            out.push_str(&format!("  - correctness: {correctness}\n"));
        }
    }
}

pub(crate) fn render_goal_block(goal: &Value, out: &mut String) {
    let vector = goal.get("vector").and_then(|v| v.as_str()).unwrap_or("");
    out.push_str("## Goal\n\n");
    if !vector.is_empty() {
        out.push_str(&format!("**Vector:** {vector}\n\n"));
    }
    render_string_list(goal, "constraints", "Constraints", out);
    render_string_list(goal, "postulates", "Postulates", out);
    if let Some(vision) = goal.get("vision").and_then(|v| v.as_str()) {
        if !vision.is_empty() {
            out.push_str("### Vision\n\n");
            out.push_str(vision);
            out.push_str("\n\n");
        }
    }
}

fn render_string_list(value: &Value, key: &str, heading: &str, out: &mut String) {
    let Some(items) = value.get(key).and_then(|v| v.as_array()) else {
        return;
    };
    if items.is_empty() {
        return;
    }
    out.push_str(&format!("### {heading}\n\n"));
    for item in items.iter().filter_map(|v| v.as_str()) {
        out.push_str(&format!("- {item}\n"));
    }
    out.push('\n');
}
