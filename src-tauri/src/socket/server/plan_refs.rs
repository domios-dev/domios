//! Ref resolution at the RPC boundary: map user-supplied plan/task/section refs
//! (full id | unique prefix | handle) onto canonical ids, with parent-free addressing.

use serde_json::Value;

use crate::service::plan::resolve::ResolveError;
use crate::service::plan::PlanStore;

use super::{get_string_param, JsonRpcError};

fn resolve_err(e: ResolveError) -> JsonRpcError {
    JsonRpcError {
        code: -32602,
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
    let reference = get_string_param(params, "plan_id")?;
    store.resolve_plan_id(&reference).map_err(resolve_err)
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
