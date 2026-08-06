use super::super::plan_task_authority::plan_owner_may_recover;
use super::{
    dependency_vec_field, dispatch, external_ref_values, validate_model_field,
    verify_owner_epoch_for_task_mutation,
};
use crate::multiplexer::state::MultiplexerState;
use crate::plan::task_update_fields::{valid_task_update_payloads, TASK_UPDATE_KEYS};
use crate::plan::{OrchestratorRef, Plan, TaskStatus};
use crate::service::plan::NewTask;
use crate::service::test_fixture::ServiceFixture;
use serde_json::json;
use std::sync::Arc;

/// The exact live owner identity the update seam authorizes against:
/// tab id, bound session and the nonce the tab itself reported back.
fn owner_params(owner: &str) -> serde_json::Map<String, serde_json::Value> {
    let mut params = serde_json::Map::new();
    params.insert("caller_tab_id".into(), json!(owner));
    params.insert("session_id".into(), json!("owner-session"));
    params.insert("session_binding_nonce".into(), json!("owner-nonce"));
    params
}

fn update_fixture() -> (ServiceFixture, String, String, String, String, String) {
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let owner = mux.create_tab(&workspace).unwrap();
    // Anchoring a Team leader does not mint its Team; owner authority needs
    // both, exactly as plan creation resolves them.
    mux.set_orchestrator_role(&owner).unwrap();
    let team_id = mux.resolve_or_mint_team_for_parent(&owner).unwrap().team_id;
    {
        let tab = mux.get_tab_mut(&owner).unwrap();
        tab.agent_session_id = Some("owner-session".into());
        tab.session_binding_nonce = Some("owner-nonce".into());
        tab.reported_session_binding_nonce = Some("owner-nonce".into());
    }
    let fixture = ServiceFixture::new(mux);
    let mut plan = Plan::new(
        "Update validation".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef {
            tab_id: Some(owner.clone()),
            session_id: Some("owner-session".into()),
            title: None,
        },
    );
    plan.team_id = Some(team_id);
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    let parent_task_id = fixture
        .service
        .plan_store
        .add_task(
            &plan.id,
            NewTask {
                name: "Parent".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let dependency_task_id = fixture
        .service
        .plan_store
        .add_task(
            &plan.id,
            NewTask {
                name: "Dependency".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            3,
        )
        .unwrap();
    let task_id = fixture
        .service
        .plan_store
        .add_task(
            &plan.id,
            NewTask {
                name: "Original".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            4,
        )
        .unwrap();
    (
        fixture,
        plan.id,
        task_id,
        parent_task_id,
        dependency_task_id,
        owner,
    )
}

fn full_task_record(fixture: &ServiceFixture, plan_id: &str, task_id: &str) -> String {
    let plan = fixture.service.plan_store.get(plan_id).unwrap();
    let task = plan.tasks.iter().find(|task| task.id == task_id).unwrap();
    serde_json::to_string_pretty(task).unwrap()
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn socket_update_rejects_unknown_keys_without_mutating_the_task() {
    for key in ["nonsense_field", "external_ref", "externalRefs"] {
        let (fixture, plan_id, task_id, _, _, owner) = update_fixture();
        let service = Arc::new(fixture.service.clone());
        let before = full_task_record(&fixture, &plan_id, &task_id);
        let mut params = serde_json::Value::Object(owner_params(&owner));
        params["plan_id"] = json!(plan_id.clone());
        params["task_id"] = json!(task_id.clone());
        params[key] = json!("ignored");
        let params = Some(params);

        let result = dispatch(
            "plan.task.update",
            &params,
            &fixture.service.app_handle,
            &service,
            99,
        );
        let after = full_task_record(&fixture, &plan_id, &task_id);
        assert!(
            result.is_err(),
            "key {key} unexpectedly succeeded\nbefore:\n{before}\nafter:\n{after}"
        );
        let error = result.unwrap_err();

        assert_eq!(error.code, -32602, "key: {key}");
        assert!(error.message.contains(key), "{}", error.message);
        assert!(error.message.contains("accepted"), "{}", error.message);
        assert!(error.message.contains("external_refs"), "{}", error.message);
        assert_eq!(error.data.as_ref().unwrap()["key"], key);
        assert_eq!(
            after, before,
            "rejected key {key} changed the full task record"
        );
    }
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn socket_every_supported_update_key_reaches_the_real_handler() {
    let (fixture, plan_id, task_id, parent_task_id, dependency_task_id, owner) = update_fixture();
    let service = Arc::new(fixture.service.clone());
    let payloads = valid_task_update_payloads(&parent_task_id, &dependency_task_id);
    assert_eq!(
        payloads.iter().map(|(key, _)| *key).collect::<Vec<_>>(),
        TASK_UPDATE_KEYS
    );

    for (index, (key, mut params)) in payloads.into_iter().enumerate() {
        params["plan_id"] = json!(plan_id.clone());
        params["task_id"] = json!(task_id.clone());
        for (name, value) in owner_params(&owner) {
            params[name] = value;
        }
        let result = dispatch(
            "plan.task.update",
            &Some(params),
            &fixture.service.app_handle,
            &service,
            100 + index as u64,
        );
        let failure = result
            .as_ref()
            .err()
            .map(|error| error.message.as_str())
            .unwrap_or("");
        assert!(
            result.is_ok(),
            "Socket failed supported key {key}: {failure}"
        );
    }
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn socket_update_external_refs_round_trips_non_null() {
    let (fixture, plan_id, task_id, _, _, owner) = update_fixture();
    let service = Arc::new(fixture.service.clone());
    let mut params = serde_json::Value::Object(owner_params(&owner));
    params["plan_id"] = json!(plan_id.clone());
    params["task_id"] = json!(task_id.clone());
    params["external_refs"] = json!(["rollacode/codeterm-releases#17"]);
    let params = Some(params);

    dispatch(
        "plan.task.update",
        &params,
        &fixture.service.app_handle,
        &service,
        99,
    )
    .unwrap();

    let plan = fixture.service.plan_store.get(&plan_id).unwrap();
    let task = plan.tasks.iter().find(|task| task.id == task_id).unwrap();
    assert_eq!(task.external_refs.len(), 1);
    assert_eq!(
        task.external_refs[0].canonical_url,
        "https://github.com/rollacode/codeterm-releases/issues/17"
    );
}

#[test]
fn rpc_dependency_field_omitted_is_none() {
    assert!(matches!(dependency_vec_field(Some(&json!({}))), Ok(None)));
}

#[test]
fn rpc_dependency_field_array_replaces() {
    assert!(matches!(
        dependency_vec_field(Some(&json!({ "depends_on": ["task_a", "task_b"] }))),
        Ok(Some(ids)) if ids == ["task_a", "task_b"]
    ));
}

#[test]
fn rpc_dependency_field_null_or_empty_clears() {
    assert!(matches!(
        dependency_vec_field(Some(&json!({ "depends_on": null }))),
        Ok(Some(ids)) if ids.is_empty()
    ));
    assert!(matches!(
        dependency_vec_field(Some(&json!({ "depends_on": [] }))),
        Ok(Some(ids)) if ids.is_empty()
    ));
}

#[test]
fn rpc_external_ref_values_require_an_array_of_strings() {
    assert_eq!(
        external_ref_values(Some(&json!({
            "external_refs": ["rollacode/codeterm-releases#17"]
        })))
        .unwrap(),
        vec!["rollacode/codeterm-releases#17".to_string()]
    );
    let error = external_ref_values(Some(&json!({ "external_refs": "#17" }))).unwrap_err();
    assert_eq!(error.code, -32602);
    assert!(error.message.contains("array of reference strings"));
}

#[test]
fn provider_id_is_rejected_in_model_field_with_canonical_syntax() {
    let providers = vec!["claude".into(), "codex".into()];
    let error = validate_model_field(Some(&json!({ "model": "claude" })), &providers).unwrap_err();
    assert_eq!(error.code, -32602);
    assert_eq!(error.data.unwrap()["type"], "provider_id_in_model_field");
    assert!(error.message.contains("<provider>:<model>"));
}
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn adopted_task_mutation_rejects_missing_stale_and_rebound_authority() {
    let (fixture, plan_id, task_id, _, _, _) = update_fixture();
    let workspace = fixture
        .service
        .mux
        .write()
        .create_workspace("owner".into(), None)
        .id;
    let tab_id = fixture.service.mux.write().create_tab(&workspace).unwrap();
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_session_id = Some("owner-session".into());
        tab.session_binding_nonce = Some("nonce".into());
        tab.reported_session_binding_nonce = Some("nonce".into());
    }
    let mut plan = fixture.service.plan_store.get(&plan_id).unwrap();
    let task = plan
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    task.interruption.owner_epoch = 2;
    task.interruption.owner_tab_id = Some(tab_id.clone());
    task.interruption.owner_session_id = Some("owner-session".into());
    fixture.service.plan_store.save(&mut plan, 5).unwrap();
    let service = Arc::new(fixture.service.clone());
    let shell_env = Some(json!({
        "plan_id": plan_id,
        "task_id": task_id,
        "caller_tab_id": tab_id,
        "session_binding_nonce": "nonce",
        "owner_epoch": 2
    }));
    assert!(verify_owner_epoch_for_task_mutation("plan.task.update", &shell_env, &service).is_ok());
    let missing = Some(json!({ "plan_id": plan_id, "task_id": task_id }));
    assert!(verify_owner_epoch_for_task_mutation("plan.task.update", &missing, &service).is_err());
    let stale = Some(json!({
        "plan_id": plan_id,
        "task_id": task_id,
        "caller_tab_id": tab_id,
        "session_id": "owner-session",
        "session_binding_nonce": "nonce",
        "owner_epoch": 1
    }));
    assert!(verify_owner_epoch_for_task_mutation("plan.task.update", &stale, &service).is_err());
    let rebound = Some(json!({
        "plan_id": plan_id,
        "task_id": task_id,
        "caller_tab_id": tab_id,
        "session_id": "other-session",
        "session_binding_nonce": "nonce",
        "owner_epoch": 2
    }));
    assert!(verify_owner_epoch_for_task_mutation("plan.task.update", &rebound, &service).is_err());
}

/// Bind an in_progress task to a worker owner (tab + session + non-zero epoch),
/// the exact shape reconcile flags `in_progress_tab_gone / needs-reassign`.
fn bind_worker_owner(
    fixture: &ServiceFixture,
    plan_id: &str,
    task_id: &str,
    owner_tab_id: &str,
    owner_session_id: &str,
    owner_epoch: u64,
) {
    let mut plan = fixture.service.plan_store.get(plan_id).unwrap();
    let task = plan
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    task.status = TaskStatus::InProgress;
    task.assigned_tab_id = Some(owner_tab_id.to_string());
    task.interruption.owner_epoch = owner_epoch;
    task.interruption.owner_tab_id = Some(owner_tab_id.to_string());
    task.interruption.owner_session_id = Some(owner_session_id.to_string());
    fixture.service.plan_store.save(&mut plan, 5).unwrap();
}

fn recovered_task(fixture: &ServiceFixture, plan_id: &str, task_id: &str) -> crate::plan::Task {
    let plan = fixture.service.plan_store.get(plan_id).unwrap();
    plan.tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap()
}

fn spawn_rebound_tab(fixture: &ServiceFixture, live_session_id: &str) -> String {
    let workspace = fixture
        .service
        .mux
        .write()
        .create_workspace("rebound".into(), None)
        .id;
    let tab_id = fixture.service.mux.write().create_tab(&workspace).unwrap();
    let mut mux = fixture.service.mux.write();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_session_id = Some(live_session_id.into());
    tab.session_binding_nonce = Some("rebound-nonce".into());
    tab.reported_session_binding_nonce = Some("rebound-nonce".into());
    tab_id
}

/// Owner SESSION dead, tab object absent from the mux: the bound Orchestrator
/// recovers, and both `close` and the reconcile-blessed `blocked/needs-reassign`
/// verb retire-and-recover in one transition (epoch bumped, owner cleared).
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn dead_owner_session_absent_tab_lets_bound_orchestrator_recover() {
    for method in ["plan.task.close", "plan.task.update"] {
        let (fixture, plan_id, task_id, _, _, owner) = update_fixture();
        bind_worker_owner(&fixture, &plan_id, &task_id, "dead-worker-tab", "worker-session", 5);
        let service = Arc::new(fixture.service.clone());

        let orch_caller = Some(serde_json::Value::Object(owner_params(&owner)));
        assert!(
            plan_owner_may_recover(
                method,
                &orch_caller,
                &service,
                &plan_id,
                Some("dead-worker-tab"),
                Some("worker-session"),
            ),
            "bound Orchestrator must recover a dead-owner task via {method}"
        );

        let mut params = owner_params(&owner);
        params.insert("plan_id".into(), json!(plan_id.clone()));
        params.insert("task_id".into(), json!(task_id.clone()));
        if method == "plan.task.update" {
            params.insert("status".into(), json!("blocked"));
            params.insert("status_label".into(), json!("needs-reassign"));
        }
        let params = Some(serde_json::Value::Object(params));
        verify_owner_epoch_for_task_mutation(method, &params, &service)
            .unwrap_or_else(|error| panic!("{method} must recover, got: {}", error.message));

        let after = recovered_task(&fixture, &plan_id, &task_id);
        assert!(after.interruption.owner_tab_id.is_none(), "{method}: owner tab cleared");
        assert!(after.interruption.owner_session_id.is_none(), "{method}: owner session cleared");
        assert!(after.interruption.owner_epoch > 5, "{method}: epoch bumped past the dead owner");
        assert!(after.assigned_tab_id.is_none(), "{method}: assignment cleared");
    }
}

/// Tab lingers in the mux but rebound to a NEW session: still a gone owner, so
/// the bound Orchestrator recovers exactly as with an absent tab.
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn lingering_tab_with_rebound_session_is_a_gone_owner() {
    let (fixture, plan_id, task_id, _, _, owner) = update_fixture();
    let rebound_tab = spawn_rebound_tab(&fixture, "new-rebound-session");
    bind_worker_owner(&fixture, &plan_id, &task_id, &rebound_tab, "worker-session", 7);
    let service = Arc::new(fixture.service.clone());

    let orch_caller = Some(serde_json::Value::Object(owner_params(&owner)));
    assert!(
        plan_owner_may_recover(
            "plan.task.close",
            &orch_caller,
            &service,
            &plan_id,
            Some(&rebound_tab),
            Some("worker-session"),
        ),
        "a tab rebound to a new session is not the live owner"
    );

    let mut params = owner_params(&owner);
    params.insert("plan_id".into(), json!(plan_id.clone()));
    params.insert("task_id".into(), json!(task_id.clone()));
    let params = Some(serde_json::Value::Object(params));
    verify_owner_epoch_for_task_mutation("plan.task.close", &params, &service).unwrap();

    let after = recovered_task(&fixture, &plan_id, &task_id);
    assert!(after.interruption.owner_tab_id.is_none());
    assert!(after.interruption.owner_epoch > 7);
}

/// The guard stays intact for a genuinely LIVE owner: the recovery gate refuses,
/// and the epoch guard still rejects a foreign caller — no clobber of a live one.
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn live_owner_session_refuses_recovery_and_keeps_the_epoch_guard() {
    let (fixture, plan_id, task_id, _, _, owner) = update_fixture();
    let live_worker = spawn_rebound_tab(&fixture, "worker-session");
    bind_worker_owner(&fixture, &plan_id, &task_id, &live_worker, "worker-session", 4);
    let service = Arc::new(fixture.service.clone());

    let orch_caller = Some(serde_json::Value::Object(owner_params(&owner)));
    assert!(
        !plan_owner_may_recover(
            "plan.task.close",
            &orch_caller,
            &service,
            &plan_id,
            Some(&live_worker),
            Some("worker-session"),
        ),
        "a live owner session must never be recoverable by a foreign caller"
    );

    let foreign = Some(json!({
        "plan_id": plan_id,
        "task_id": task_id,
        "caller_tab_id": owner,
        "session_id": "owner-session",
        "session_binding_nonce": "owner-nonce",
        "owner_epoch": 4
    }));
    assert!(
        verify_owner_epoch_for_task_mutation("plan.task.update", &foreign, &service).is_err(),
        "the epoch guard must still fence a foreign caller off a live owner"
    );

    let after = recovered_task(&fixture, &plan_id, &task_id);
    assert_eq!(after.interruption.owner_epoch, 4, "the live owner's epoch is untouched");
    assert_eq!(after.interruption.owner_tab_id.as_deref(), Some(live_worker.as_str()));
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn standalone_task_add_does_not_require_an_existing_task_owner() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let service = Arc::new(fixture.service.clone());
    let params = Some(json!({ "name": "Inbox item", "type": "task" }));

    assert!(verify_owner_epoch_for_task_mutation("task.add", &params, &service).is_ok());
}
