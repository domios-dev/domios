use serde_json::json;

use crate::plan::Task;
use crate::service::plan::PlanError;

use super::JsonRpcError;

pub(super) fn preflight_review_capacity(
    plan: &crate::plan::Plan,
    task: &Task,
) -> Result<(), PlanError> {
    crate::service::plan::review_gate::validate_dispatch_review_capacity(plan, task)
}

pub(super) fn preflight_dispatch_task_status(
    plan_id: &str,
    task: &Task,
) -> Result<(), JsonRpcError> {
    if task.status != crate::plan::TaskStatus::Paused {
        return Ok(());
    }
    let fix = format!("codeterm plan task resume {plan_id} {}", task.id);
    Err(JsonRpcError {
        code: -32602,
        message: format!("task '{}' is paused\nfix: {fix}", task.id),
        data: Some(json!({
            "type": "invalid_task_dispatch",
            "stage": "task_status",
            "reason": "task is paused",
            "fix": fix,
        })),
    })
}
