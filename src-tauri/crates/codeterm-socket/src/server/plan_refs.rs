//! Ref resolution at the RPC boundary: map user-supplied plan/task/section refs
//! (full id | unique prefix | handle) onto canonical ids, with parent-free addressing.

use serde_json::Value;

use codeterm_service_core::service::plan::resolve::ResolveError;
use codeterm_service_core::service::plan::PlanStore;

use super::error_codes::{INVALID_PARAMS, NOT_FOUND};
use super::{get_string_param, JsonRpcError};

fn resolve_err(e: ResolveError) -> JsonRpcError {
    let code = match &e {
        ResolveError::Read(error) => error.rpc_code(),
        ResolveError::NotFound { .. } => NOT_FOUND,
        ResolveError::Ambiguous { .. } => INVALID_PARAMS,
    };
    JsonRpcError {
        code,
        message: e.to_string(),
        data: Some(e.to_json()),
    }
}

fn optional_plan_ref(params: &Option<Value>) -> Option<String> {
    params
        .as_ref()
        .and_then(|p| p.get("plan_id"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Required `plan_id` param resolved to a canonical id (full id | prefix).
pub(super) fn resolve_plan(
    store: &PlanStore,
    params: &Option<Value>,
) -> Result<String, JsonRpcError> {
    if let Some(reference) = optional_plan_ref(params) {
        return store.resolve_plan_id(&reference).map_err(resolve_err);
    }

    let caller_tab = params
        .as_ref()
        .and_then(|value| value.get("caller_tab_id"))
        .and_then(|value| value.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("");

    if let Some(binding) = store
        .assigned_task_binding_projection(caller_tab)
        .map_err(super::plan::plan_rpc_err)?
    {
        return Ok(binding.plan_id);
    }
    if let Some(plan_id) = store
        .active_owned_plan_with_open_work_for_tab(caller_tab)
        .map_err(super::plan::plan_rpc_err)?
    {
        return Ok(plan_id);
    }

    let (code, message, kind) = if caller_tab.is_empty() {
        (
            INVALID_PARAMS,
            "plan_id is required: pass it as an explicit positional arg".to_string(),
            "plan_id_required",
        )
    } else {
        (
            NOT_FOUND,
            format!("no active plan bound to Tab '{caller_tab}'; pass the plan id explicitly"),
            "plan_binding_not_found",
        )
    };
    Err(JsonRpcError {
        code,
        message,
        data: Some(serde_json::json!({
            "type": kind,
            "tab_id": caller_tab,
        })),
    })
}

/// Task-scoped: `plan_id` optional, owning plan resolved from the task ref when absent.
pub(super) fn resolve_task(
    store: &PlanStore,
    params: &Option<Value>,
) -> Result<(String, String), JsonRpcError> {
    let task_ref = get_string_param(params, "task_id")?;
    let plan_ref = optional_plan_ref(params);
    let resolved = store
        .resolve_task_ref(plan_ref.as_deref(), &task_ref)
        .map_err(resolve_err)?;
    Ok((resolved.plan_id, resolved.task_id))
}

pub(super) fn task_binding(
    store: &PlanStore,
    params: &Option<Value>,
    team_id: Option<&str>,
) -> Result<Value, JsonRpcError> {
    let tab_id = get_string_param(params, "tab_id")?;
    let binding = store
        .assigned_task_binding_projection(&tab_id)
        .map_err(super::plan::plan_rpc_err)?
        .map(|binding| {
            serde_json::json!({
                "plan_id": binding.plan_id,
                "task_id": binding.task_id,
            })
        })
        .or_else(|| {
            let (plan_id, _) = store.active_plan_for_team(team_id?)?;
            Some(serde_json::json!({
                "plan_id": plan_id,
                "task_id": null,
            }))
        });
    Ok(serde_json::json!({
        "tab_id": tab_id,
        "binding": binding,
    }))
}

#[cfg(test)]
mod tests;

/// Required `spec_id` param resolved to a canonical id (full id | prefix).
pub(super) fn resolve_spec(
    store: &PlanStore,
    params: &Option<Value>,
) -> Result<String, JsonRpcError> {
    let reference = get_string_param(params, "spec_id")?;
    store.resolve_spec_id(&reference).map_err(resolve_err)
}

/// Spec-section-scoped: resolve `spec_id` then the section ref within that spec.
pub(super) fn resolve_spec_section(
    store: &PlanStore,
    params: &Option<Value>,
) -> Result<(String, String), JsonRpcError> {
    let spec_ref = get_string_param(params, "spec_id")?;
    let section_ref = get_string_param(params, "section_id")?;
    store
        .resolve_spec_section_id(&spec_ref, &section_ref)
        .map_err(resolve_err)
}

/// Section-scoped: `plan_id` optional, owning plan resolved from the section ref when absent.
pub(super) fn resolve_section(
    store: &PlanStore,
    params: &Option<Value>,
) -> Result<(String, String), JsonRpcError> {
    let section_ref = get_string_param(params, "section_id")?;
    let plan_ref = optional_plan_ref(params);
    let resolved = store
        .resolve_section_ref(plan_ref.as_deref(), &section_ref)
        .map_err(resolve_err)?;
    Ok((resolved.plan_id, resolved.section_id))
}

pub(super) fn resolve_loop(
    store: &PlanStore,
    params: &Option<Value>,
) -> Result<String, JsonRpcError> {
    let reference = get_string_param(params, "loop_id")?;
    store.resolve_loop_id(&reference).map_err(resolve_err)
}

pub(super) fn resolve_element(
    store: &PlanStore,
    params: &Option<Value>,
) -> Result<(String, String), JsonRpcError> {
    let plan_id = resolve_plan(store, params)?;
    let element_ref = get_string_param(params, "element_id")?;
    store
        .resolve_element_ref(&plan_id, &element_ref)
        .map_err(resolve_err)
}

pub(super) fn resolve_milestone(
    store: &PlanStore,
    params: &Option<Value>,
) -> Result<(String, String, String), JsonRpcError> {
    let milestone_ref = get_string_param(params, "milestone_id")?;
    let task_ref = get_string_param(params, "task_id")?;
    let plan_ref = optional_plan_ref(params);
    let resolved = store
        .resolve_milestone_ref(plan_ref.as_deref(), &task_ref, &milestone_ref)
        .map_err(resolve_err)?;
    Ok((resolved.plan_id, resolved.task_id, resolved.milestone_id))
}
