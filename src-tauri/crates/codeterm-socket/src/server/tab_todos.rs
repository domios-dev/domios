//! Bounded JSON-RPC surface for service-owned tab checklists.

use super::{get_string_param, opt_string_param, JsonRpcError};
use codeterm_service_core::service::ServiceState;
use codeterm_types::agent::{TabTodoDraft, TabTodoState, MAX_TAB_TODO_ITEMS};
use serde::de::DeserializeOwned;
use serde_json::Value;

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    let target = get_string_param(params, "tab_id")?;
    let actor = opt_string_param(params, "caller_tab_id");
    let result = match method {
        "tab.todo.create" => {
            let actor_id = actor.as_deref().ok_or_else(|| {
                invalid("caller_tab_id is required for checklist dispatch seeding")
            })?;
            let criteria: Vec<String> = bounded_array(params, "acceptance_criteria")?;
            let checklist = codeterm_service_core::async_runtime::block_on(
                svc.tab_todo_seed_from_acceptance(&target, actor_id, &criteria),
            )
            .map_err(service_error)?;
            serde_json::to_value(checklist)
        }
        "tab.todo.append" => {
            let actor_id = actor
                .as_deref()
                .ok_or_else(|| invalid("caller_tab_id is required"))?;
            let drafts: Vec<TabTodoDraft> = bounded_array(params, "items")?;
            let generation = number_param(params, "generation")?;
            let checklist = codeterm_service_core::async_runtime::block_on(
                svc.tab_todo_append_qa(&target, actor_id, generation, drafts),
            )
            .map_err(service_error)?;
            serde_json::to_value(checklist)
        }
        "tab.todo.state" => {
            let generation = number_param(params, "generation")?;
            let expected_revision = number_param(params, "expected_revision")?;
            let item_id = get_string_param(params, "item_id")?;
            let state = parse_state(params)?;
            let checklist = codeterm_service_core::async_runtime::block_on(svc.tab_todo_set_state(
                &target,
                actor.as_deref(),
                generation,
                item_id,
                expected_revision,
                state,
            ))
            .map_err(service_error)?;
            serde_json::to_value(checklist)
        }
        "tab.todo.amend" => {
            let actor_id = actor
                .as_deref()
                .ok_or_else(|| invalid("caller_tab_id is required"))?;
            let generation = number_param(params, "generation")?;
            let expected_revision = number_param(params, "expected_revision")?;
            let item_id = get_string_param(params, "item_id")?;
            let draft: TabTodoDraft = object_param(params, "item")?;
            let checklist = codeterm_service_core::async_runtime::block_on(svc.tab_todo_amend(
                &target,
                actor_id,
                generation,
                item_id,
                expected_revision,
                draft,
            ))
            .map_err(service_error)?;
            serde_json::to_value(checklist)
        }
        "tab.todo.clear_one" => {
            let generation = number_param(params, "generation")?;
            let expected_revision = number_param(params, "expected_revision")?;
            let item_id = get_string_param(params, "item_id")?;
            let checklist = codeterm_service_core::async_runtime::block_on(svc.tab_todo_clear_one(
                &target,
                actor.as_deref(),
                generation,
                item_id,
                expected_revision,
            ))
            .map_err(service_error)?;
            serde_json::to_value(checklist)
        }
        "tab.todo.clear_all" => {
            let generation = number_param(params, "generation")?;
            let checklist = codeterm_service_core::async_runtime::block_on(svc.tab_todo_clear_all(
                &target,
                actor.as_deref(),
                generation,
            ))
            .map_err(service_error)?;
            serde_json::to_value(checklist)
        }
        "tab.todo.reset" => {
            let generation = number_param(params, "generation")?;
            let drafts: Vec<TabTodoDraft> = bounded_array(params, "items")?;
            let checklist = codeterm_service_core::async_runtime::block_on(svc.tab_todo_reset(
                &target,
                actor.as_deref(),
                generation,
                drafts,
            ))
            .map_err(service_error)?;
            serde_json::to_value(checklist)
        }
        "tab.todo.list" => {
            let checklist = codeterm_service_core::async_runtime::block_on(
                svc.tab_todo_list(&target, actor.as_deref()),
            )
            .map_err(service_error)?;
            serde_json::to_value(checklist)
        }
        "tab.todo.parent_list" => {
            let parent = actor
                .as_deref()
                .ok_or_else(|| invalid("caller_tab_id is required"))?;
            let checklist = codeterm_service_core::async_runtime::block_on(
                svc.tab_todo_parent_list(&target, parent),
            )
            .map_err(service_error)?;
            serde_json::to_value(checklist)
        }
        "tab.todo.summary" => {
            let parent = actor
                .as_deref()
                .ok_or_else(|| invalid("caller_tab_id is required"))?;
            let summary = codeterm_service_core::async_runtime::block_on(
                svc.tab_todo_parent_summary(&target, parent),
            )
            .map_err(service_error)?;
            serde_json::to_value(summary)
        }
        "tab.todo.recite" => {
            let recitation = codeterm_service_core::async_runtime::block_on(
                svc.tab_todo_recite(&target, actor.as_deref()),
            )
            .map_err(service_error)?;
            serde_json::to_value(recitation)
        }
        _ => {
            return Err(JsonRpcError {
                code: -32601,
                message: format!("Unknown tab todo method: {method}"),
                data: None,
            })
        }
    };
    result.map_err(|error| JsonRpcError {
        code: -32603,
        message: error.to_string(),
        data: None,
    })
}

#[cfg(test)]
mod tests;

fn bounded_array<T: DeserializeOwned>(
    params: &Option<Value>,
    name: &str,
) -> Result<T, JsonRpcError> {
    let value = params
        .as_ref()
        .and_then(|value| value.get(name))
        .ok_or_else(|| invalid(format!("{name} is required")))?;
    let array = value
        .as_array()
        .ok_or_else(|| invalid(format!("{name} must be an array")))?;
    if array.is_empty() || array.len() > MAX_TAB_TODO_ITEMS {
        return Err(invalid(format!(
            "{name} must contain 1..={MAX_TAB_TODO_ITEMS} items"
        )));
    }
    serde_json::from_value(value.clone()).map_err(|error| invalid(error.to_string()))
}

fn object_param<T: DeserializeOwned>(
    params: &Option<Value>,
    name: &str,
) -> Result<T, JsonRpcError> {
    let value = params
        .as_ref()
        .and_then(|value| value.get(name))
        .cloned()
        .ok_or_else(|| invalid(format!("{name} is required")))?;
    serde_json::from_value(value).map_err(|error| invalid(error.to_string()))
}

fn number_param(params: &Option<Value>, name: &str) -> Result<u64, JsonRpcError> {
    params
        .as_ref()
        .and_then(|value| value.get(name))
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid(format!("{name} must be an unsigned integer")))
}

fn parse_state(params: &Option<Value>) -> Result<TabTodoState, JsonRpcError> {
    let value = params
        .as_ref()
        .and_then(|value| value.get("state"))
        .cloned()
        .ok_or_else(|| invalid("state is required"))?;
    serde_json::from_value(value).map_err(|error| invalid(error.to_string()))
}

fn invalid(message: impl Into<String>) -> JsonRpcError {
    JsonRpcError {
        code: -32602,
        message: message.into(),
        data: None,
    }
}

fn service_error(message: String) -> JsonRpcError {
    JsonRpcError {
        code: -32000,
        message,
        data: None,
    }
}
