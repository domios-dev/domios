use std::collections::{BTreeMap, HashMap};

use crate::client::RpcClient;
use crate::cmd::plan::{call_plan_rpc, render_empty_reason};
use crate::cmd::task_list_render::{build_task_tree, task_str, TaskSummaryRow};
use serde_json::{json, Value};

const IN_PROGRESS_STATUSES: [&str; 3] = ["in_progress", "in_review", "blocked"];

pub(crate) fn run(
    client: &mut dyn RpcClient,
    json_output: bool,
    plan_id: Option<String>,
    all: bool,
) -> Result<Option<Value>, String> {
    let explicit_plan = plan_id.is_some();
    let mut params = json!({
        "active": true,
        "mine": false,
        "unassigned": false,
    });
    if let Some(plan_id) = plan_id {
        params["plan_id"] = json!(plan_id);
    }
    if !explicit_plan && !all && !client.is_remote() {
        if let Some(project) = std::env::current_dir()
            .ok()
            .map(|path| path.display().to_string())
        {
            params["project"] = json!(project);
        }
    }

    let response = call_plan_rpc(client, "task.list", params)?;
    let tasks: Vec<Value> = response
        .get("tasks")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter(|item| task_str(item, "status").is_some_and(is_in_progress_status))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let mut val = json!({
        "statuses": IN_PROGRESS_STATUSES,
        "tasks": tasks,
    });
    if let Some(reason) = response.get("empty_reason") {
        val["empty_reason"] = reason.clone();
        let message = render_empty_reason(reason).unwrap_or_else(|| {
            let kind = reason
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("unknown_scope_reason");
            format!("The task scope returned no active tasks ({kind}).")
        });
        val["task_list_message"] = json!(message);
    }
    if json_output {
        return Ok(Some(val));
    }
    print!("{}", render_in_progress_tasks(&val));
    Ok(None)
}

fn is_in_progress_status(status: &str) -> bool {
    IN_PROGRESS_STATUSES.contains(&status)
}

fn lane_str<'a>(item: &'a Value, field: &str) -> Option<&'a str> {
    TaskSummaryRow::new(item).lane_str(field)
}

fn task_sort_key(item: &Value) -> (&str, &str) {
    (
        task_str(item, "name").unwrap_or(""),
        task_str(item, "id").unwrap_or(""),
    )
}

fn render_in_progress_tasks(val: &Value) -> String {
    let Some(tasks) = val.get("tasks").and_then(|v| v.as_array()) else {
        return "(no active tasks)\n".into();
    };
    if tasks.is_empty() {
        if let Some(message) = val.get("task_list_message").and_then(Value::as_str) {
            return format!("{message}\n");
        }
        return "(no active tasks)\n".into();
    }

    let mut by_plan: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
    for item in tasks {
        let plan_id = item
            .get("plan_id")
            .and_then(|v| v.as_str())
            .unwrap_or("?")
            .to_string();
        by_plan.entry(plan_id).or_default().push(item);
    }

    let mut out = String::new();
    for (plan_id, mut plan_tasks) in by_plan {
        plan_tasks.sort_by_key(|item| task_sort_key(item));
        let plan_title = plan_tasks
            .first()
            .and_then(|item| item.get("plan_title"))
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        out.push_str(&format!("{plan_id}\t{plan_title}\n"));
        render_in_progress_plan_tasks(&mut out, &plan_tasks);
    }
    out
}

fn render_in_progress_plan_tasks(out: &mut String, tasks: &[&Value]) {
    let tree = build_task_tree(tasks);
    for item in tree.roots {
        render_in_progress_task(out, item, 1, &tree.children);
    }
}

fn render_in_progress_task(
    out: &mut String,
    item: &Value,
    depth: usize,
    children: &HashMap<&str, Vec<&Value>>,
) {
    let row = TaskSummaryRow::new(item);
    let id = row.id;
    let status = row.status;
    let assignee = task_str(item, "assigned_tab_id")
        .or_else(|| lane_str(item, "tab_id"))
        .unwrap_or("-");
    let workspace = lane_str(item, "workspace_id").unwrap_or("-");
    let branch = lane_str(item, "branch").unwrap_or("-");
    let cleanup = lane_str(item, "cleanup_status").unwrap_or("-");
    let name = row.name;
    out.push_str(&format!(
        "{}{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
        "  ".repeat(depth),
        id,
        status,
        assignee,
        workspace,
        branch,
        cleanup,
        name
    ));
    if let Some(items) = children.get(id) {
        for child in items {
            render_in_progress_task(out, child, depth + 1, children);
        }
    }
}

#[cfg(test)]
mod tests;
