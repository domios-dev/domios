//! A review bounce may name the failed checklist step; it is reopened through
//! the same service operation as `codeterm tab todo reopen`.

use codeterm_service_core::plan::AgentRef;
use codeterm_service_core::service::ServiceState;
use serde_json::Value;

use super::super::JsonRpcError;

/// Runs before the bounce is recorded so an unknown item refuses the bounce
/// without changing task state.
pub(super) fn reopen_named_item(
    svc: &ServiceState,
    params: Option<&Value>,
    plan_id: &str,
    task_id: &str,
    from: &AgentRef,
    note: &str,
) -> Result<(), JsonRpcError> {
    let Some(item_id) = params
        .and_then(|params| params.get("todo_item"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| !item.is_empty())
    else {
        return Ok(());
    };
    let executor = svc
        .plan_store
        .get(plan_id)
        .and_then(|plan| {
            plan.tasks
                .into_iter()
                .find(|task| task.id == task_id)
                .and_then(|task| task.assigned_tab_id)
        })
        .filter(|tab| !tab.trim().is_empty())
        .ok_or_else(|| refusal("the bounced task has no assigned Worker tab to reopen"))?;
    codeterm_service_core::async_runtime::block_on(svc.tab_todo_reopen_for_review(
        &executor,
        from.tab_id.as_deref(),
        item_id.to_owned(),
        note,
    ))
    .map(|_| ())
    .map_err(|error| refusal(&format!("todo item {item_id} was not reopened: {error}")))
}

fn refusal(message: &str) -> JsonRpcError {
    JsonRpcError {
        code: -32602,
        message: message.to_owned(),
        data: Some(serde_json::json!({ "type": "bounce_todo_reopen_refused" })),
    }
}
