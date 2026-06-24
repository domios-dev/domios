use crate::client::SocketCallError;
use serde_json::Value;

pub(crate) fn render_plan_call_error(err: SocketCallError, params: &Value) -> String {
    match err {
        SocketCallError::Rpc(rpc) => rpc
            .data
            .as_ref()
            .and_then(|data| render_plan_error(data, params))
            .unwrap_or_else(|| format!("RPC error {}: {}", rpc.code, rpc.message)),
        SocketCallError::Transport(message) => message,
    }
}

fn render_plan_error(data: &Value, params: &Value) -> Option<String> {
    let kind = data.get("type")?.as_str()?;
    let plan_id = params
        .get("plan_id")
        .and_then(|v| v.as_str())
        .unwrap_or("<id>");
    let task_id = params.get("task_id").and_then(|v| v.as_str());
    let loop_id = params
        .get("loop_id")
        .and_then(|v| v.as_str())
        .unwrap_or("<loop_id>");
    match kind {
        "undeclared_kind" => render_undeclared_kind(data, plan_id, task_id),
        "bad_status" => Some(format!(
            "✗ invalid task status.\nvalid statuses: pending|in_progress|blocked|done|cancelled\nfix: codeterm plan task update {plan_id} {} --status pending\nuse --status-label for free-text nuance.",
            task_id.unwrap_or("<task_id>")
        )),
        "not_found" => Some(format!(
            "✗ no plan '{plan_id}' was found.\nfix: codeterm plan list --active"
        )),
        "task_not_found" => Some(format!(
            "✗ no task '{}' was found in plan '{plan_id}'.\nfix: codeterm plan list --active",
            task_id.unwrap_or("<task_id>")
        )),
        "open_subtasks" => render_open_subtasks(data, plan_id, task_id),
        "budget_exhausted" => {
            let count = data.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
            let budget = data.get("budget").and_then(|v| v.as_u64()).unwrap_or(0);
            Some(format!(
                "✗ loop budget {count}/{budget} reached for loop '{loop_id}'; extend with codeterm plan loop set {loop_id} --budget N, or close the plan."
            ))
        }
        _ => None,
    }
}

fn render_undeclared_kind(data: &Value, plan_id: &str, task_id: Option<&str>) -> Option<String> {
    let declared = data
        .get("declared")
        .and_then(|v| v.as_array())
        .map(|items| items.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
        .unwrap_or_default();
    let listed = if declared.is_empty() {
        "(none)".to_string()
    } else {
        declared.join("|")
    };
    let csv = if declared.is_empty() {
        "<new>".to_string()
    } else {
        format!("{},<new>", declared.join(","))
    };
    Some(format!(
        "✗ undeclared milestone kind.\nvalid kinds: {listed}\nfix: codeterm plan kinds set {plan_id} --kinds {csv}\nor declare and use it once with: codeterm plan milestone add {plan_id} {} --kind <new> --add-kind --text='status update'",
        task_id.unwrap_or("<task_id>")
    ))
}

fn render_open_subtasks(data: &Value, plan_id: &str, task_id: Option<&str>) -> Option<String> {
    let subtasks = data
        .get("subtasks")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(render_open_subtask)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let listed = if subtasks.is_empty() {
        "- <unknown subtask>".to_string()
    } else {
        subtasks.join("\n")
    };
    Some(format!(
        "✗ task '{}' still has open subtasks:\n{listed}\nfix: close or cancel the child tasks first, then retry:\n  codeterm plan task close {plan_id} <child_task_id>\n  codeterm plan task close {plan_id} {}",
        task_id.unwrap_or("<task_id>"),
        task_id.unwrap_or("<task_id>")
    ))
}

fn render_open_subtask(item: &Value) -> Option<String> {
    let id = item.get("id").and_then(|v| v.as_str())?;
    let name = item
        .get("name")
        .and_then(|v| v.as_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(id);
    let status = item
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    Some(format!("- {name} ({id}) [{status}]"))
}
