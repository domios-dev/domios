use serde_json::Value;
use std::collections::HashMap;

use crate::cmd::task_list_render::build_task_tree;

pub(crate) fn render_plan_markdown(plan: &Value) -> String {
    let mut out = String::new();
    let title = plan.get("title").and_then(|v| v.as_str()).unwrap_or("?");
    let id = plan.get("id").and_then(|v| v.as_str()).unwrap_or("?");
    let status = plan.get("status").and_then(|v| v.as_str()).unwrap_or("?");
    out.push_str(&format!("# {title} (`{id}`)\n\n"));
    out.push_str("> Exported snapshot. CodeTerm planner state is authoritative; update the plan with `codeterm plan ...`, not by editing this markdown.\n\n");
    out.push_str(&format!("**Status:** {status}\n\n"));
    if let Some(path) = plan
        .get("canonical_project_path")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
    {
        out.push_str(&format!("**Canonical project path:** `{path}`\n\n"));
    }
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
            render_plan_tasks(tasks, &mut out);
            out.push('\n');
        }
    }
    out
}

fn render_plan_tasks(tasks: &[Value], out: &mut String) {
    let refs: Vec<&Value> = tasks.iter().collect();
    let tree = build_task_tree(&refs);
    for task in tree.roots {
        append_plan_task(out, task, 0, &tree.children);
    }
}

fn append_plan_task(
    out: &mut String,
    task: &Value,
    depth: usize,
    children: &HashMap<&str, Vec<&Value>>,
) {
    let indent = "  ".repeat(depth);
    let name = task.get("name").and_then(|v| v.as_str()).unwrap_or("?");
    let tid = task.get("id").and_then(|v| v.as_str()).unwrap_or("?");
    let st = task.get("status").and_then(|v| v.as_str()).unwrap_or("?");
    let role_suffix = task
        .get("executor_role")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|r| format!(" [role:{r}]"))
        .unwrap_or_default();
    out.push_str(&format!(
        "{indent}- **{name}** (`{tid}`) — {st}{role_suffix}\n"
    ));
    render_task_competition(task, depth, out);
    render_task_lane(task, depth, out);
    render_task_quality(task, depth, out);
    if let Some(id) = task.get("id").and_then(|v| v.as_str()) {
        if let Some(kids) = children.get(id) {
            for child in kids {
                append_plan_task(out, child, depth + 1, children);
            }
        }
    }
}

fn render_task_competition(task: &Value, depth: usize, out: &mut String) {
    let Some(competition) = task.get("competition").filter(|v| !v.is_null()) else {
        return;
    };
    if competition.get("enabled").and_then(|v| v.as_bool()) != Some(true) {
        return;
    }
    let indent = "  ".repeat(depth + 1);
    let attempts = competition
        .get("attempts")
        .and_then(|v| v.as_u64())
        .unwrap_or(2)
        .max(2);
    let models = competition
        .get("models")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();
    let mut parts = vec![format!("{attempts} attempts")];
    if !models.is_empty() {
        parts.push(format!("models {models}"));
    }
    if let Some(hint) = competition.get("judge_hint").and_then(|v| v.as_str()) {
        parts.push(format!("judge: {hint}"));
    }
    out.push_str(&format!("{indent}- competition: {}\n", parts.join("; ")));
}

fn render_task_lane(task: &Value, depth: usize, out: &mut String) {
    let Some(lane) = task.get("lane").and_then(|v| v.as_object()) else {
        return;
    };
    let indent = "  ".repeat(depth + 1);
    let mut parts = vec![];
    push_lane_part(&mut parts, lane.get("tab_id"), "tab");
    push_lane_part(&mut parts, lane.get("workspace_id"), "workspace");
    push_lane_part(&mut parts, lane.get("workspace_name"), "workspace name");
    push_lane_part(&mut parts, lane.get("worktree_path"), "worktree");
    push_lane_part(&mut parts, lane.get("branch"), "branch");
    push_lane_part(&mut parts, lane.get("base"), "base");
    push_lane_part(&mut parts, lane.get("destination"), "destination");
    push_lane_part(&mut parts, lane.get("cleanup_status"), "cleanup");
    if !parts.is_empty() {
        out.push_str(&format!("{indent}- lane: {}\n", parts.join("; ")));
    }
    if let Some(hints) = lane.get("cleanup_hints").and_then(|v| v.as_array()) {
        for hint in hints.iter().filter_map(|v| v.as_str()) {
            out.push_str(&format!("{indent}- cleanup hint: {hint}\n"));
        }
    }
}

fn push_lane_part(parts: &mut Vec<String>, value: Option<&Value>, label: &str) {
    if let Some(text) = value.and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
        parts.push(format!("{label}: {text}"));
    }
}

fn render_task_quality(task: &Value, depth: usize, out: &mut String) {
    let indent = "  ".repeat(depth + 1);
    if let Some(items) = task.get("acceptance_criteria").and_then(|v| v.as_array()) {
        for item in items.iter().filter_map(|v| v.as_str()) {
            out.push_str(&format!("{indent}- criterion: {item}\n"));
        }
    }
    if let Some(test_plan) = task.get("test_plan").and_then(|v| v.as_str()) {
        if !test_plan.is_empty() {
            out.push_str(&format!("{indent}- test plan: {test_plan}\n"));
        }
    }
    if let Some(qa_plan) = task.get("qa_plan").and_then(|v| v.as_str()) {
        if !qa_plan.is_empty() {
            out.push_str(&format!("{indent}- QA plan: {qa_plan}\n"));
        }
    }
    if let Some(correctness) = task.get("correctness").and_then(|v| v.as_str()) {
        if !correctness.is_empty() {
            out.push_str(&format!("{indent}- correctness: {correctness}\n"));
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
