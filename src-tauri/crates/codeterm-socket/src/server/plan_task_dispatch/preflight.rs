use serde_json::json;

use codeterm_service_core::plan::Task;
use codeterm_service_core::service::plan::PlanError;

use super::JsonRpcError;

pub(super) fn preflight_review_capacity(
    plan: &codeterm_service_core::plan::Plan,
    task: &Task,
) -> Result<(), PlanError> {
    codeterm_service_core::service::plan::review_gate::validate_dispatch_review_capacity(plan, task)
}

pub(super) fn preflight_dispatch_task_status(
    _plan_id: &str,
    task: &Task,
) -> Result<(), JsonRpcError> {
    match &task.status {
        codeterm_service_core::plan::TaskStatus::Pending
        | codeterm_service_core::plan::TaskStatus::InProgress
        | codeterm_service_core::plan::TaskStatus::Blocked => Ok(()),
        codeterm_service_core::plan::TaskStatus::Paused => Err(JsonRpcError {
            code: -32602,
            message: format!(
                "task '{}' is held by the system and cannot be dispatched",
                task.id
            ),
            data: Some(json!({
                "type": "invalid_task_dispatch",
                "stage": "task_status",
                "reason": "task is paused",
                "hold_kind": paused_hold_kind(task),
                "pause_reason": task.pause_reason.as_deref(),
                "status_label": task.status_label.as_deref(),
            })),
        }),
        status => Err(super::plan_rpc_err(PlanError::TransitionDenied {
            from: status.clone(),
            to: codeterm_service_core::plan::TaskStatus::Pending,
        })),
    }
}

fn paused_hold_kind(task: &Task) -> &'static str {
    if task.status_label.as_deref() == Some("provider_exhausted") {
        "provider_exhausted"
    } else if task
        .pause_reason
        .as_deref()
        .is_some_and(|reason| reason.starts_with("gate:"))
    {
        "decision_gate"
    } else {
        "unclassified"
    }
}
