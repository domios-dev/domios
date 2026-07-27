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
    if let Some(reference) = optional_plan_ref(params) {
        return store.resolve_plan_id(&reference).map_err(resolve_err);
    }

    let caller_pane = params
        .as_ref()
        .and_then(|value| value.get("caller_pane_id"))
        .and_then(|value| value.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("");

    if let Some((plan_id, _)) = store.task_binding_for_pane(caller_pane) {
        return Ok(plan_id);
    }
    if let Some(plan_id) = store.active_owned_plan_with_open_work_for_pane(caller_pane) {
        return Ok(plan_id);
    }

    let message = if caller_pane.is_empty() {
        "plan_id is required: pass it as an explicit positional arg".to_string()
    } else {
        format!("no active plan bound to Tab '{caller_pane}'; pass the plan id explicitly",)
    };
    Err(JsonRpcError {
        code: -32602,
        message,
        data: Some(serde_json::json!({
            "type": "plan_binding_not_found",
            "pane_id": caller_pane,
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

#[cfg(test)]
mod tests {
    use crate::plan::{OrchestratorRef, PlanMode};
    use crate::service::plan::PlanStore;
    use serde_json::json;

    fn store_with_plan(
        dir: &std::path::Path,
        owner: &str,
        title: &str,
        mode: PlanMode,
    ) -> (PlanStore, String) {
        let store = PlanStore::new_for_test(dir);
        let plan = store
            .create_plan_for_team(
                title.into(),
                dir.display().to_string(),
                OrchestratorRef::from_pane(owner),
                format!("team-{owner}"),
                vec![],
                1,
            )
            .unwrap();
        store.set_mode(&plan.id, mode, 2).expect("mode set");
        (store, plan.id)
    }

    fn add_task(store: &PlanStore, plan_id: &str, name: &str, handle: &str) -> String {
        store
            .add_task(
                plan_id,
                crate::service::plan::NewTask {
                    name: name.into(),
                    kind: "task".into(),
                    handle: Some(handle.into()),
                    competition: None,
                    ..Default::default()
                },
                3,
            )
            .unwrap()
    }

    #[test]
    fn explicit_plan_id_wins_for_pane_bound_context() {
        let dir = tempfile::tempdir().unwrap();
        let (store, explicit_id) = store_with_plan(
            dir.path(),
            "owner_explicit",
            "Explicit",
            PlanMode::Execution,
        );
        let bound_id = store
            .create_plan_for_team(
                "Bound".into(),
                dir.path().display().to_string(),
                OrchestratorRef::from_pane("owner_bound"),
                "team-owner-bound".into(),
                vec![],
                1,
            )
            .unwrap()
            .id;
        store.set_mode(&bound_id, PlanMode::Execution, 2).unwrap();
        let task_id = add_task(&store, &bound_id, "bound", "bound");
        store
            .set_task_assignment(&task_id, Some("pane-bound"), None, 4)
            .unwrap();

        let params = json!({
            "plan_id": explicit_id,
            "caller_pane_id": "pane-bound"
        });
        let resolved = super::resolve_plan(&store, &Some(params))
            .unwrap_or_else(|err| panic!("{}", err.message));
        assert_eq!(resolved, explicit_id);
    }

    #[test]
    fn resolve_plan_uses_task_binding_for_pane() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id) =
            store_with_plan(dir.path(), "owner_task", "Task plan", PlanMode::Execution);
        let task_id = add_task(&store, &plan_id, "pane-task", "pane-task");
        store
            .set_task_assignment(&task_id, Some("pane-task"), None, 4)
            .unwrap();

        let params = json!({ "caller_pane_id": "pane-task" });
        let resolved = super::resolve_plan(&store, &Some(params))
            .unwrap_or_else(|err| panic!("{}", err.message));
        assert_eq!(resolved, plan_id);
    }

    #[test]
    fn resolve_plan_uses_owned_open_work_plan_for_pane() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id) = store_with_plan(
            dir.path(),
            "owner_active",
            "Owner plan",
            PlanMode::Execution,
        );
        let _task_id = add_task(&store, &plan_id, "open-work", "open-work");

        let params = json!({ "caller_pane_id": "owner_active" });
        let resolved = super::resolve_plan(&store, &Some(params))
            .unwrap_or_else(|err| panic!("{}", err.message));
        assert_eq!(resolved, plan_id);
    }

    #[test]
    fn resolve_plan_reports_actionable_error_when_unbound() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = store_with_plan(
            dir.path(),
            "owner_active",
            "Owner plan",
            PlanMode::Execution,
        );
        let params = json!({ "caller_pane_id": "unbound-pane" });
        let err = super::resolve_plan(&store, &Some(params)).unwrap_err();
        assert_eq!(err.code, -32602);
        assert!(err.message.contains("pass the plan id explicitly"));
        assert_eq!(
            err.data,
            Some(serde_json::json!({
                "type": "plan_binding_not_found",
                "pane_id": "unbound-pane"
            }))
        );
    }
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
