use super::{INVALID_PARAMS, NOT_FOUND};
use codeterm_service_core::plan::{OrchestratorRef, PlanMode};
use codeterm_service_core::service::plan::PlanStore;
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
            codeterm_service_core::service::plan::NewTask {
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
    let resolved =
        super::resolve_plan(&store, &Some(params)).unwrap_or_else(|err| panic!("{}", err.message));
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
    let resolved =
        super::resolve_plan(&store, &Some(params)).unwrap_or_else(|err| panic!("{}", err.message));
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
    let resolved =
        super::resolve_plan(&store, &Some(params)).unwrap_or_else(|err| panic!("{}", err.message));
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
    let (store, _) = store_with_plan(dir.path(), "owner_none", "Owner plan", PlanMode::Execution);

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
    let (store, plan_id) = store_with_plan(dir.path(), "owner", "Bound plan", PlanMode::Execution);
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
    let (store, plan_id) = store_with_plan(dir.path(), "owner", "Team plan", PlanMode::Execution);
    let binding = super::task_binding(
        &store,
        &Some(serde_json::json!({"tab_id": "team-planner"})),
        Some("team-owner"),
    )
    .unwrap();
    assert_eq!(binding["binding"]["plan_id"], plan_id);
    assert_eq!(binding["binding"]["task_id"], serde_json::Value::Null);
}

#[test]
fn storage_reference_failures_keep_internal_code_and_typed_plan_error() {
    use codeterm_service_core::service::plan::{resolve::ResolveError, PlanError};
    let cause = PlanError::Io("opaque storage failure".into());
    let expected = cause.to_json();
    let error = super::resolve_err(ResolveError::Read(cause));
    assert_eq!(
        error.code,
        codeterm_service_core::service::plan::RPC_INTERNAL_ERROR
    );
    assert_eq!(error.data, Some(expected));
    assert!(error.message.contains("opaque storage failure"));
}
