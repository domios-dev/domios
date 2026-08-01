use std::path::Path;

use serde_json::Value;

use super::{plan_id, Finding};

#[path = "../../../../src-tauri/src/plan/cleanup_debt.rs"]
mod cleanup_debt;

pub(super) fn cleanup_debt_findings(path: &Path, plan: &Value, task: &Value) -> Vec<Finding> {
    let plan_id = plan_id(plan, path);
    let Some(task_id) = task.get("id").and_then(Value::as_str) else {
        return Vec::new();
    };
    let task_name = task.get("name").and_then(Value::as_str).unwrap_or(task_id);
    let task_status = task.get("status").and_then(Value::as_str).unwrap_or("");
    let lane = task.get("lane").and_then(Value::as_object);
    let cleanup_hints = lane
        .and_then(|lane| lane.get("cleanup_hints"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let project_path = plan
        .get("canonical_project_path")
        .or_else(|| plan.get("project_path"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let worktree_path = lane
        .and_then(|lane| lane.get("worktree_path"))
        .and_then(Value::as_str);
    let worktree_exists = worktree_path.is_some_and(|worktree| Path::new(worktree).exists());
    let item = cleanup_debt::detect_cleanup_debt(&cleanup_debt::CleanupDebtInput {
        plan_id: plan_id.as_deref().unwrap_or(""),
        task_id,
        task_name,
        task_status,
        is_terminal: matches!(task_status, "done" | "cancelled"),
        cleanup_status: lane
            .and_then(|lane| lane.get("cleanup_status"))
            .and_then(Value::as_str),
        cleanup_hints: &cleanup_hints,
        tab_id: lane
            .and_then(|lane| lane.get("tab_id"))
            .and_then(Value::as_str),
        tab_live: false,
        worktree_path,
        worktree_exists,
        branch: lane
            .and_then(|lane| lane.get("branch"))
            .and_then(Value::as_str),
        project_path,
    });
    let Some(item) = item else {
        return Vec::new();
    };
    vec![Finding {
        severity: "warning",
        kind: if item.deferred {
            "cleanup_deferred"
        } else {
            "cleanup_debt"
        },
        plan_id,
        path: path.display().to_string(),
        task_id: Some(task_id.to_string()),
        detail: item
            .issues
            .iter()
            .map(|issue| issue.detail.as_str())
            .collect::<Vec<_>>()
            .join("; "),
        fix: item.commands.join(" ; "),
    }]
}
