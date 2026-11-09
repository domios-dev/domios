use super::{dispatch, owner_params, update_fixture};
use codeterm_service_core::multiplexer::types::AgentRole;
use codeterm_service_core::plan::{Plan, PlannerRef};
use codeterm_service_core::service::test_fixture::ServiceFixture;
use serde_json::{json, Value};
use std::sync::Arc;

fn fixture() -> (ServiceFixture, Plan, String, String) {
    let (fixture, plan_id, task_id, _, _, owner) = update_fixture();
    let mut plan = fixture.service.plan_store.get(&plan_id).unwrap();
    plan.sections =
        serde_json::from_value(json!([{"id":"sec_relink","title":"Original"}])).unwrap();
    let task = plan
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    task.section_ids.clear();
    task.interruption.owner_epoch = 7;
    task.interruption.owner_tab_id = Some("gone-worker".into());
    fixture.service.plan_store.save(&mut plan, 10).unwrap();
    (fixture, plan, task_id, owner)
}

fn params(owner: &str, plan: &str, apply: bool) -> Option<Value> {
    let mut params = Value::Object(owner_params(owner));
    params["plan_id"] = json!(plan);
    params["apply"] = json!(apply);
    Some(params)
}

#[test]
fn relink_plan_preview_is_read_only_and_owner_apply_needs_no_task_reference() {
    let (fixture, plan, task, owner) = fixture();
    let service = Arc::new(fixture.service.clone());
    let preview = dispatch(
        "plan.task.relink",
        &params(&owner, &plan.id, false),
        &service,
        20,
    )
    .unwrap();
    assert_eq!(preview["applied"], json!([]));
    assert_eq!(preview["suggestions"][0]["task_id"], task);
    assert_eq!(preview["suggestions"][0]["section_id"], "sec_relink");
    assert_eq!(service.plan_store.get(&plan.id).unwrap(), plan);
    let applied = dispatch(
        "plan.task.relink",
        &params(&owner, &plan.id, true),
        &service,
        21,
    )
    .unwrap();
    assert_eq!(applied["applied"][0]["task_id"], task);
    let persisted = service.plan_store.get(&plan.id).unwrap();
    let persisted_task = persisted
        .tasks
        .iter()
        .find(|candidate| candidate.id == task)
        .unwrap();
    assert_eq!(persisted_task.section_ids, ["sec_relink"]);
    assert_eq!(persisted_task.interruption.owner_epoch, 7);
    assert_eq!(
        persisted_task.interruption.owner_tab_id.as_deref(),
        Some("gone-worker")
    );
}

#[test]
fn relink_plan_allows_bound_planner_and_refuses_stale_owner_or_planner_without_writes() {
    for planner in [false, true] {
        let (fixture, mut plan, _, owner) = fixture();
        let caller = if planner {
            let mut mux = fixture.service.mux.write();
            let workspace = mux.find_workspace_for_tab(&owner).unwrap();
            let tab_id = mux.create_tab(&workspace).unwrap();
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.role = Some(AgentRole::Manager);
            tab.role_profile = Some("planner".into());
            tab.team_id = plan.team_id.clone();
            tab.parent_tab_id = Some(owner.clone());
            tab.agent_session_id = Some("owner-session".into());
            tab.session_binding_nonce = Some("owner-nonce".into());
            tab.reported_session_binding_nonce = Some("owner-nonce".into());
            plan.planner = Some(PlannerRef::Tab {
                tab_id: tab_id.clone(),
                session_id: Some("owner-session".into()),
                title: None,
            });
            tab_id
        } else {
            owner
        };
        fixture.service.plan_store.save(&mut plan, 11).unwrap();
        let service = Arc::new(fixture.service.clone());
        for apply in [false, true] {
            let before = service.plan_store.get(&plan.id).unwrap();
            let mut stale = params(&caller, &plan.id, apply).unwrap();
            stale["session_binding_nonce"] = json!("stale");
            let error = dispatch("plan.task.relink", &Some(stale), &service, 20).unwrap_err();
            assert_eq!(error.code, -32003);
            assert_eq!(service.plan_store.get(&plan.id).unwrap(), before);
        }
        let applied = dispatch(
            "plan.task.relink",
            &params(&caller, &plan.id, true),
            &service,
            21,
        )
        .unwrap();
        assert_eq!(applied["applied"].as_array().unwrap().len(), 1);
    }
}

#[test]
fn relink_plan_refuses_authenticated_foreign_caller_for_preview_and_apply() {
    let (fixture, plan, _, owner) = fixture();
    let foreign = {
        let mut mux = fixture.service.mux.write();
        let workspace = mux.find_workspace_for_tab(&owner).unwrap();
        let id = mux.create_tab(&workspace).unwrap();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.agent_session_id = Some("owner-session".into());
        tab.session_binding_nonce = Some("owner-nonce".into());
        tab.reported_session_binding_nonce = Some("owner-nonce".into());
        id
    };
    let service = Arc::new(fixture.service.clone());
    for apply in [false, true] {
        let error = dispatch(
            "plan.task.relink",
            &params(&foreign, &plan.id, apply),
            &service,
            20,
        )
        .unwrap_err();
        assert_eq!(error.code, -32003);
        assert_eq!(service.plan_store.get(&plan.id).unwrap(), plan);
    }
}
