//! Ref resolution at the RPC boundary: map user-supplied plan/task/section refs
//! (full id | unique prefix | handle) onto canonical ids, with parent-free addressing.

use serde_json::Value;

use crate::service::plan::resolve::ResolveError;
use crate::service::plan::PlanStore;

use super::error_codes::{INVALID_PARAMS, NOT_FOUND};
use super::{get_string_param, JsonRpcError};

fn resolve_err(e: ResolveError) -> JsonRpcError {
    let code = match &e {
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

    if let Some((plan_id, _)) = store.task_binding_for_tab(caller_tab) {
        return Ok(plan_id);
    }
    if let Some(plan_id) = store.active_owned_plan_with_open_work_for_tab(caller_tab) {
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
        .task_binding_for_tab(&tab_id)
        .map(|(plan_id, task_id)| {
            serde_json::json!({
                "plan_id": plan_id,
                "task_id": task_id,
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
mod tests {
    use super::{INVALID_PARAMS, NOT_FOUND};
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
                OrchestratorRef::from_tab(owner),
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
    fn explicit_plan_id_wins_for_tab_bound_context() {
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
                OrchestratorRef::from_tab("owner_bound"),
                "team-owner-bound".into(),
                vec![],
                1,
            )
            .unwrap()
            .id;
        store.set_mode(&bound_id, PlanMode::Execution, 2).unwrap();
        let task_id = add_task(&store, &bound_id, "bound", "bound");
        store
            .set_task_assignment(&task_id, Some("tab-bound"), None, 4)
            .unwrap();

        let params = json!({
            "plan_id": explicit_id,
            "caller_tab_id": "tab-bound"
        });
        let resolved = super::resolve_plan(&store, &Some(params))
            .unwrap_or_else(|err| panic!("{}", err.message));
        assert_eq!(resolved, explicit_id);
    }

    #[test]
    fn resolve_plan_uses_task_binding_for_tab() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id) =
            store_with_plan(dir.path(), "owner_task", "Task plan", PlanMode::Execution);
        let task_id = add_task(&store, &plan_id, "tab-task", "tab-task");
        store
            .set_task_assignment(&task_id, Some("tab-task"), None, 4)
            .unwrap();

        let params = json!({ "caller_tab_id": "tab-task" });
        let resolved = super::resolve_plan(&store, &Some(params))
            .unwrap_or_else(|err| panic!("{}", err.message));
        assert_eq!(resolved, plan_id);
    }

    #[test]
    fn resolve_plan_uses_owned_open_work_plan_for_tab() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id) = store_with_plan(
            dir.path(),
            "owner_active",
            "Owner plan",
            PlanMode::Execution,
        );
        let _task_id = add_task(&store, &plan_id, "open-work", "open-work");

        let params = json!({ "caller_tab_id": "owner_active" });
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
        let params = json!({ "caller_tab_id": "unbound-tab" });
        let err = super::resolve_plan(&store, &Some(params)).unwrap_err();
        assert_eq!(err.code, NOT_FOUND);
        assert!(err.message.contains("pass the plan id explicitly"));
        assert_eq!(
            err.data,
            Some(serde_json::json!({
                "type": "plan_binding_not_found",
                "tab_id": "unbound-tab"
            }))
        );
    }

    #[test]
    fn missing_plan_id_without_a_caller_tab_stays_invalid_params() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) =
            store_with_plan(dir.path(), "owner_none", "Owner plan", PlanMode::Execution);

        let err = super::resolve_plan(&store, &Some(json!({}))).unwrap_err();
        assert_eq!(err.code, INVALID_PARAMS);
        assert_eq!(err.data.unwrap()["type"], "plan_id_required");
    }

    #[test]
    fn unknown_plan_ref_is_not_found_not_invalid_params() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id) =
            store_with_plan(dir.path(), "owner_ref", "Owner plan", PlanMode::Execution);

        let params = json!({ "plan_id": format!("{plan_id}_nope") });
        let err = super::resolve_plan(&store, &Some(params)).unwrap_err();
        assert_eq!(err.code, NOT_FOUND);
        let data = err.data.unwrap();
        assert_eq!(data["type"], "ref_not_found");
        assert_eq!(data["kind"], "plan");
    }

    #[test]
    fn unknown_task_ref_is_not_found_not_invalid_params() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id) = store_with_plan(
            dir.path(),
            "owner_task_ref",
            "Owner plan",
            PlanMode::Execution,
        );
        let task_id = add_task(&store, &plan_id, "real", "real");

        let params = json!({ "plan_id": plan_id, "task_id": format!("{task_id}_nope") });
        let err = super::resolve_task(&store, &Some(params)).unwrap_err();
        assert_eq!(err.code, NOT_FOUND);
        let data = err.data.unwrap();
        assert_eq!(data["type"], "ref_not_found");
        assert_eq!(data["kind"], "task");
    }

    #[test]
    fn ambiguous_plan_ref_stays_invalid_params() {
        let dir = tempfile::tempdir().unwrap();
        let (store, first) = store_with_plan(dir.path(), "owner_a", "First", PlanMode::Execution);
        let second = store
            .create_plan_for_team(
                "Second".into(),
                dir.path().display().to_string(),
                OrchestratorRef::from_tab("owner_b"),
                "team-owner-b".into(),
                vec![],
                1,
            )
            .unwrap()
            .id;
        let shared: String = first
            .chars()
            .zip(second.chars())
            .take_while(|(a, b)| a == b)
            .map(|(a, _)| a)
            .collect();
        assert!(!shared.is_empty(), "minted ids must share a typed prefix");

        let err = super::resolve_plan(&store, &Some(json!({ "plan_id": shared }))).unwrap_err();
        assert_eq!(err.code, INVALID_PARAMS);
        assert_eq!(err.data.unwrap()["type"], "ref_ambiguous");
    }

    #[test]
    fn not_found_and_invalid_params_never_collapse_into_one_code() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id) =
            store_with_plan(dir.path(), "owner_split", "Owner plan", PlanMode::Execution);

        let missing = super::resolve_plan(
            &store,
            &Some(json!({ "plan_id": format!("{plan_id}_nope") })),
        )
        .unwrap_err();
        let malformed = super::resolve_plan(&store, &Some(json!({}))).unwrap_err();

        assert_eq!(missing.code, NOT_FOUND);
        assert_eq!(malformed.code, INVALID_PARAMS);
        assert_ne!(missing.code, malformed.code);
    }

    #[test]
    fn task_binding_returns_exact_bound_task_and_unknown_stays_null() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id) =
            store_with_plan(dir.path(), "owner", "Bound plan", PlanMode::Execution);
        let task_id = add_task(&store, &plan_id, "bound", "bound");
        store
            .set_task_assignment(&task_id, Some("tab-bound"), None, 4)
            .unwrap();

        let bound = super::task_binding(
            &store,
            &Some(serde_json::json!({"tab_id": "tab-bound"})),
            None,
        )
        .unwrap();
        assert_eq!(bound["binding"]["plan_id"], plan_id);
        assert_eq!(bound["binding"]["task_id"], task_id);

        let unknown = super::task_binding(
            &store,
            &Some(serde_json::json!({"tab_id": "tab-unbound"})),
            None,
        )
        .unwrap();
        assert_eq!(unknown["tab_id"], "tab-unbound");
        assert_eq!(unknown["binding"], serde_json::Value::Null);
    }

    #[test]
    fn team_member_binding_resolves_the_team_active_plan_without_a_task() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id) =
            store_with_plan(dir.path(), "owner", "Team plan", PlanMode::Execution);
        let binding = super::task_binding(
            &store,
            &Some(serde_json::json!({"tab_id": "team-planner"})),
            Some("team-owner"),
        )
        .unwrap();
        assert_eq!(binding["binding"]["plan_id"], plan_id);
        assert_eq!(binding["binding"]["task_id"], serde_json::Value::Null);
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
