use super::super::plan_task_authority::plan_owner_may_recover;
use super::{
    dependency_vec_field, dispatch, dispatch_with_spawn, external_ref_values, validate_model_field,
    verify_owner_epoch_for_task_mutation,
};
use crate::multiplexer::state::MultiplexerState;
use crate::plan::task_update_fields::{valid_task_update_payloads, TASK_UPDATE_KEYS};
use crate::plan::{OrchestratorRef, Plan, TaskLaneMetadata, TaskStatus};
use crate::service::plan::NewTask;
use crate::service::test_fixture::ServiceFixture;
use serde_json::json;
use std::sync::{Arc, Barrier, Mutex};

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
fn split_creates_children_with_parent_and_inherited_routing() {
    let (fixture, plan_id, _, _, _, owner) = update_fixture();
    let parent_id = fixture.service.plan_store.add_task(
        &plan_id,
        NewTask {
            name: "Split parent".into(),
            kind: "implement".into(),
            model: Some("codex:gpt-6-sol".into()),
            model_rationale: Some("precise edits".into()),
            personality: Some("Nova".into()),
            role_guidance: Some("Own the Rust layer.".into()),
            executor_role: Some("rust-backend-dev".into()),
            assigned_tab_id: Some("tab-parent-worker".into()),
            lane: TaskLaneMetadata { tab_id: Some("tab-parent-worker".into()), ..Default::default() },
            body: "Shared context".into(),
            acceptance_criteria: vec!["works".into()],
            test_plan: Some("focused test".into()),
            correctness: Some("preserve isolation".into()),
            ..Default::default()
        },
        5,
    ).unwrap();
    fixture.service.plan_store.with_plan(&plan_id, 6, |plan| {
        let parent = plan.tasks.iter_mut().find(|task| task.id == parent_id).unwrap();
        parent.status = TaskStatus::InProgress;
        parent.status_changed_at = 6;
        Ok(())
    }).unwrap();
    let service = Arc::new(fixture.service.clone());
    let mut params = serde_json::Value::Object(owner_params(&owner));
    params["plan_id"] = json!(plan_id.clone());
    params["task_id"] = json!(parent_id.clone());
    params["children"] = json!([
        {"name":"Part A"},
        {"name":"Part B", "body":"Part-specific context", "criteria":["part works"], "test_plan":"verify this part"}
    ]);

    let response = dispatch(
        "plan.task.split",
        &Some(params),
        &fixture.service.app_handle,
        &service,
        10,
    ).unwrap();
    assert_eq!(response["parent_task_id"], parent_id);
    let children = response["children"].as_array().unwrap();
    assert_eq!(children.len(), 2);
    let plan = fixture.service.plan_store.get(&plan_id).unwrap();
    let parent = plan.tasks.iter().find(|task| task.id == parent_id).unwrap();
    assert_eq!(parent.status, TaskStatus::InProgress);
    assert_eq!(parent.assigned_tab_id.as_deref(), Some("tab-parent-worker"));
    assert_eq!(parent.lane.tab_id.as_deref(), Some("tab-parent-worker"));
    for (index, created) in children.iter().enumerate() {
        let id = created["task_id"].as_str().unwrap();
        let child = plan.tasks.iter().find(|task| task.id == id).unwrap();
        assert_eq!(child.parent_task_id.as_deref(), Some(parent_id.as_str()));
        assert_eq!(child.executor_role.as_deref(), Some("rust-backend-dev"));
        assert_eq!(child.personality.as_deref(), Some("Nova"));
        assert_eq!(child.model.as_deref(), Some("codex:gpt-6-sol"));
        assert!(child.assigned_tab_id.is_none());
        assert!(child.lane.tab_id.is_none());
        if index == 0 {
            assert_eq!(child.body, "Shared context");
            assert_eq!(child.acceptance_criteria, vec!["works".to_string()]);
            assert_eq!(child.test_plan.as_deref(), Some("focused test"));
        } else {
            assert_eq!(child.body, "Part-specific context");
            assert_eq!(child.acceptance_criteria, vec!["part works".to_string()]);
            assert_eq!(child.test_plan.as_deref(), Some("verify this part"));
        }
    }
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn split_invalid_second_child_is_atomic() {
    let (fixture, plan_id, _, _, _, owner) = update_fixture();
    let parent_id = fixture.service.plan_store.add_task(&plan_id, NewTask { name: "Parent".into(), executor_role: Some("rust-backend-dev".into()), ..Default::default() }, 5).unwrap();
    let before = fixture.service.plan_store.get(&plan_id).unwrap().tasks.len();
    let service = Arc::new(fixture.service.clone());
    let mut params = serde_json::Value::Object(owner_params(&owner));
    params["plan_id"] = json!(plan_id.clone());
    params["task_id"] = json!(parent_id);
    params["children"] = json!([{"name":"Valid"}, {"name":"Invalid", "sections":"missing-section"}]);
    assert!(dispatch("plan.task.split", &Some(params), &fixture.service.app_handle, &service, 10).is_err());
    assert_eq!(fixture.service.plan_store.get(&plan_id).unwrap().tasks.len(), before);
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn split_rejects_terminal_and_child_parents() {
    for status in [Some(TaskStatus::Done), Some(TaskStatus::Cancelled), None] {
        let (fixture, plan_id, _, _, _, owner) = update_fixture();
        let parent_id = fixture.service.plan_store.add_task(&plan_id, NewTask { name: "Parent".into(), executor_role: Some("rust-backend-dev".into()), ..Default::default() }, 5).unwrap();
        let target_id = if let Some(status) = status {
            fixture.service.plan_store.with_plan(&plan_id, 6, |plan| {
                plan.tasks.iter_mut().find(|task| task.id == parent_id).unwrap().status = status;
                Ok(())
            }).unwrap();
            parent_id
        } else {
            fixture.service.plan_store.add_task(&plan_id, NewTask { name: "Child".into(), parent_task_id: Some(parent_id), ..Default::default() }, 6).unwrap()
        };
        let service = Arc::new(fixture.service.clone());
        let mut params = serde_json::Value::Object(owner_params(&owner));
        params["plan_id"] = json!(plan_id);
        params["task_id"] = json!(target_id);
        params["children"] = json!([{"name":"Nested"}]);
        assert!(dispatch("plan.task.split", &Some(params), &fixture.service.app_handle, &service, 10).is_err());
    }
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn draft_split_children_remain_locked_until_plan_approval() {
    let (fixture, plan_id, _, _, _, owner) = update_fixture();
    let parent_id = fixture.service.plan_store.add_task(&plan_id, NewTask { name: "Parent".into(), executor_role: Some("rust-backend-dev".into()), ..Default::default() }, 5).unwrap();
    let service = Arc::new(fixture.service.clone());
    let mut params = serde_json::Value::Object(owner_params(&owner));
    params["plan_id"] = json!(plan_id.clone());
    params["task_id"] = json!(parent_id);
    params["children"] = json!([{"name":"Child"}]);
    let response = dispatch("plan.task.split", &Some(params), &fixture.service.app_handle, &service, 10).unwrap();
    let child_id = response["children"][0]["task_id"].as_str().unwrap();
    let lane = TaskLaneMetadata { tab_id: Some("child-worker".into()), ..Default::default() };
    assert!(fixture.service.plan_store.begin_task_dispatch(&plan_id, child_id, "child-worker", "Nova", lane.clone(), 11).is_err());
    fixture.service.plan_store.set_mode(&plan_id, crate::plan::PlanMode::Approved, 12).unwrap();
    assert!(fixture.service.plan_store.begin_task_dispatch(&plan_id, child_id, "child-worker", "Nova", lane, 13).is_ok());
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

/// codeterm#372: no replace flag on the wire means append.
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn socket_update_acceptance_criteria_defaults_to_append() {
    let (fixture, plan_id, task_id, _, _, owner) = update_fixture();
    fixture
        .service
        .plan_store
        .update_task_quality(
            &plan_id,
            &task_id,
            Some(
                crate::service::plan::mutations::task_ops::AcceptanceCriteriaUpdate::Replace(vec![
                    "existing one".into(),
                    "existing two".into(),
                ]),
            ),
            None,
            None,
            None,
            50,
        )
        .unwrap();
    let service = Arc::new(fixture.service.clone());
    let mut params = serde_json::Value::Object(owner_params(&owner));
    params["plan_id"] = json!(plan_id.clone());
    params["task_id"] = json!(task_id.clone());
    params["acceptance_criteria"] = json!(["new one"]);

    let response = dispatch(
        "plan.task.update",
        &Some(params),
        &fixture.service.app_handle,
        &service,
        99,
    )
    .unwrap();

    assert!(
        response.get("acceptance_criteria_counts").is_none(),
        "an append must not report replacement counts: {response}"
    );
    let plan = fixture.service.plan_store.get(&plan_id).unwrap();
    let task = plan.tasks.iter().find(|task| task.id == task_id).unwrap();
    assert_eq!(
        task.acceptance_criteria,
        vec!["existing one", "existing two", "new one"]
    );
}

/// Replace reports before/after counts — never a silent overwrite.
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn socket_update_acceptance_criteria_replace_reports_before_after_counts() {
    let (fixture, plan_id, task_id, _, _, owner) = update_fixture();
    fixture
        .service
        .plan_store
        .update_task_quality(
            &plan_id,
            &task_id,
            Some(
                crate::service::plan::mutations::task_ops::AcceptanceCriteriaUpdate::Replace(vec![
                    "existing one".into(),
                    "existing two".into(),
                ]),
            ),
            None,
            None,
            None,
            50,
        )
        .unwrap();
    let service = Arc::new(fixture.service.clone());
    let mut params = serde_json::Value::Object(owner_params(&owner));
    params["plan_id"] = json!(plan_id.clone());
    params["task_id"] = json!(task_id.clone());
    params["acceptance_criteria"] = json!(["replacement"]);
    params["acceptance_criteria_replace"] = json!(true);

    let response = dispatch(
        "plan.task.update",
        &Some(params),
        &fixture.service.app_handle,
        &service,
        99,
    )
    .unwrap();

    assert_eq!(
        response["acceptance_criteria_counts"],
        json!({ "before": 2, "after": 1 })
    );
    let plan = fixture.service.plan_store.get(&plan_id).unwrap();
    let task = plan.tasks.iter().find(|task| task.id == task_id).unwrap();
    assert_eq!(task.acceptance_criteria, vec!["replacement"]);
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

fn starting_worker_with_live_pty(
    fixture: &ServiceFixture,
    plan_id: &str,
    task_id: &str,
    owner_tab: &str,
) -> (String, String, String) {
    let worker_tab = {
        let mut mux = fixture.service.mux.write();
        let workspace = mux.find_workspace_for_tab(owner_tab).unwrap();
        mux.create_tab(&workspace).unwrap()
    };
    let (worker_pty, _) = fixture
        .service
        .pty
        .lock()
        .spawn(
            Some("cmd.exe".into()),
            None,
            80,
            24,
            std::collections::HashMap::new(),
        )
        .unwrap();
    {
        let mut mux = fixture.service.mux.write();
        let worker = mux.get_tab_mut(&worker_tab).unwrap();
        worker.pty_id = Some(worker_pty.clone());
        worker.agent_type = Some("test-provider".into());
        worker.agent_booting = true;
        worker.task_binding = Some(crate::multiplexer::types::TaskBinding {
            plan_id: Some(plan_id.to_string()),
            task_id: Some(task_id.to_string()),
            spawn_handle: None,
            bound_at: 5,
        });
    }
    let attempt_id = fixture
        .service
        .plan_store
        .begin_task_dispatch(
            plan_id,
            task_id,
            &worker_tab,
            "Worker",
            TaskLaneMetadata::default(),
            5,
        )
        .unwrap();
    (worker_tab, worker_pty, attempt_id)
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn destination_update_during_startup_keeps_attempt_tab_and_lane() {
    let (fixture, plan_id, task_id, _, _, owner) = update_fixture();
    let (worker_tab, worker_pty, attempt_id) =
        starting_worker_with_live_pty(&fixture, &plan_id, &task_id, &owner);
    let mut params = owner_params(&owner);
    params.insert("plan_id".into(), json!(plan_id.clone()));
    params.insert("task_id".into(), json!(task_id.clone()));
    params.insert("owner_epoch".into(), json!(1));
    params.insert("lane".into(), json!({"destination": "release/1.10.1"}));
    let params = Some(serde_json::Value::Object(params));
    let service = Arc::new(fixture.service.clone());

    dispatch(
        "plan.task.update",
        &params,
        &fixture.service.app_handle,
        &service,
        6,
    )
    .expect("integration metadata update should preserve a live startup attempt");

    let starting = fixture.service.plan_store.show_task(&task_id).unwrap().task;
    assert_eq!(
        starting.assigned_tab_id.as_deref(),
        Some(worker_tab.as_str())
    );
    assert_eq!(starting.lane.tab_id.as_deref(), Some(worker_tab.as_str()));
    assert_eq!(starting.lane.destination.as_deref(), Some("release/1.10.1"));
    assert_eq!(starting.execution_attempts.len(), 1);
    assert_eq!(starting.execution_attempts[0].id, attempt_id);
    assert_eq!(starting.execution_attempts[0].closed_at, None);
    assert_eq!(starting.execution_attempts[0].acknowledged_at, None);

    fixture
        .service
        .plan_store
        .finish_task_dispatch(
            &plan_id,
            &task_id,
            &worker_tab,
            &attempt_id,
            "worker-session",
            "worker-nonce",
            TaskLaneMetadata::default(),
            7,
        )
        .unwrap();
    fixture
        .service
        .plan_store
        .acknowledge_task_attempt(&plan_id, &task_id, &worker_tab, &attempt_id, 8)
        .unwrap();

    let acknowledged = fixture.service.plan_store.show_task(&task_id).unwrap().task;
    assert_eq!(
        acknowledged.assigned_tab_id.as_deref(),
        Some(worker_tab.as_str())
    );
    assert_eq!(
        acknowledged.lane.tab_id.as_deref(),
        Some(worker_tab.as_str())
    );
    assert_eq!(
        acknowledged.lane.destination.as_deref(),
        Some("release/1.10.1")
    );
    assert_eq!(acknowledged.execution_attempts[0].closed_at, None);
    assert_eq!(acknowledged.execution_attempts[0].acknowledged_at, Some(8));
    fixture.service.pty.lock().kill(&worker_pty).unwrap();
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn dispatch_refuses_live_startup_owner_before_spawning_another_worker() {
    let (fixture, plan_id, task_id, _, _, owner) = update_fixture();
    fixture
        .service
        .plan_store
        .set_planning_state(
            &plan_id,
            crate::plan::PlanningState::Approved,
            crate::plan::PlanningAuthor::User,
            None,
            5,
        )
        .unwrap();
    let (worker_tab, worker_pty, attempt_id) =
        starting_worker_with_live_pty(&fixture, &plan_id, &task_id, &owner);
    let service = Arc::new(fixture.service.clone());
    let params = Some(json!({
        "plan_id": plan_id.clone(),
        "task_id": task_id.clone(),
        "caller_tab_id": owner.clone(),
        "session_id": "owner-session",
        "session_binding_nonce": "owner-nonce"
    }));
    let tabs_before = fixture.service.mux.read().snapshot().tabs.len();

    let refusal = super::super::plan_task_dispatch::dispatch(
        &params,
        &fixture.service.app_handle,
        &service,
        6,
    )
    .expect_err("a second dispatch must be refused before spawning");

    assert_eq!(refusal.code, crate::service::plan::RPC_DISPATCH_IN_FLIGHT);
    assert_eq!(refusal.data.as_ref().unwrap()["type"], "dispatch_in_flight");
    assert!(refusal.message.contains(&attempt_id));
    assert!(refusal.message.contains(&worker_tab));
    assert_eq!(
        fixture.service.mux.read().snapshot().tabs.len(),
        tabs_before
    );
    let task = fixture.service.plan_store.show_task(&task_id).unwrap().task;
    assert_eq!(task.assigned_tab_id.as_deref(), Some(worker_tab.as_str()));
    assert_eq!(task.execution_attempts.len(), 1);
    assert_eq!(task.execution_attempts[0].closed_at, None);
    assert!(fixture
        .service
        .pty
        .lock()
        .get_session(&worker_pty)
        .is_some());
    fixture.service.pty.lock().kill(&worker_pty).unwrap();
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn dispatch_spawn_failure_closes_attempt_and_reserved_tab() {
    let (fixture, plan_id, task_id, _, _, owner) = update_fixture();
    fixture
        .service
        .plan_store
        .set_planning_state(
            &plan_id,
            crate::plan::PlanningState::Approved,
            crate::plan::PlanningAuthor::User,
            None,
            5,
        )
        .unwrap();
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("dispatch-failure-provider"),
    ));
    let mut plan = fixture.service.plan_store.get(&plan_id).unwrap();
    plan.tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap()
        .execution_intent = crate::plan::TaskExecutionIntent::NonRepository;
    fixture.service.plan_store.save(&mut plan, 6).unwrap();

    let service = Arc::new(fixture.service.clone());
    let tabs_before = fixture.service.mux.read().snapshot().tabs.len();
    let params = Some(json!({
        "plan_id": plan_id.clone(),
        "task_id": task_id.clone(),
        "caller_tab_id": owner,
        "session_id": "owner-session",
        "session_binding_nonce": "owner-nonce",
        "agent": "dispatch-failure-provider",
        "execution_intent": "read_only"
    }));

    let error = dispatch_with_spawn(
        &params,
        &fixture.service.app_handle,
        &service,
        7,
        |_, _, _| Err("provider rejected launch".into()),
        || {},
    )
    .expect_err("a definitive provider spawn failure rolls back the dispatch");

    assert_eq!(error.data.as_ref().unwrap()["stage"], "agent_spawn");
    assert!(error.message.contains("provider rejected launch"));
    assert_eq!(
        fixture.service.mux.read().snapshot().tabs.len(),
        tabs_before
    );
    let failed = fixture.service.plan_store.show_task(&task_id).unwrap().task;
    assert_eq!(failed.status, TaskStatus::Pending);
    assert!(failed.assigned_tab_id.is_none());
    assert_eq!(failed.execution_attempts.len(), 1);
    assert_eq!(failed.execution_attempts[0].closed_at, Some(7));
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn concurrent_dispatch_of_the_same_task_is_refused_while_one_is_in_flight() {
    let (fixture, plan_id, task_id, _, _, owner) = update_fixture();
    fixture
        .service
        .plan_store
        .set_planning_state(
            &plan_id,
            crate::plan::PlanningState::Approved,
            crate::plan::PlanningAuthor::User,
            None,
            5,
        )
        .unwrap();
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("dispatch-test-provider"),
    ));
    let mut plan = fixture.service.plan_store.get(&plan_id).unwrap();
    plan.tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap()
        .execution_intent = crate::plan::TaskExecutionIntent::NonRepository;
    fixture.service.plan_store.save(&mut plan, 6).unwrap();

    let service = Arc::new(fixture.service.clone());
    let spawned_tabs = Arc::new(Mutex::new(Vec::new()));
    let ready_senders = Arc::new(Mutex::new(std::collections::HashMap::new()));
    let observations = Arc::new(Barrier::new(2));
    let tabs_before = fixture.service.mux.read().snapshot().tabs.len();
    let params = Arc::new(Some(json!({
        "plan_id": plan_id.clone(),
        "task_id": task_id.clone(),
        "caller_tab_id": owner.clone(),
        "session_id": "owner-session",
        "session_binding_nonce": "owner-nonce",
        "agent": "dispatch-test-provider",
        "execution_intent": "read_only"
    })));
    let starts = [0, 1].map(|_| {
        let service = service.clone();
        let app = fixture.service.app_handle.clone();
        let params = params.clone();
        let spawned_tabs = spawned_tabs.clone();
        let ready_senders = ready_senders.clone();
        let observations = observations.clone();
        std::thread::spawn(move || {
            dispatch_with_spawn(
                params.as_ref(),
                &app,
                &service,
                7,
                move |svc, spawn_params, ready_tx| {
                    let plan_id = spawn_params.plan_id.clone().unwrap();
                    let task_id = spawn_params.task_id.clone().unwrap();
                    let parent_tab_id = spawn_params.parent_tab_id.clone().unwrap();
                    let agent = spawn_params.agent.clone().unwrap();
                    let title = spawn_params
                        .title
                        .clone()
                        .unwrap_or_else(|| "Worker".into());
                    let role = spawn_params.role.clone();
                    let role_profile = spawn_params.role_profile.clone();
                    let description = spawn_params.description.clone();
                    let tab_id = spawn_params.existing_tab_id.clone().unwrap();
                    {
                        let mut mux = svc.mux.write();
                        let tab = mux.get_tab_mut(&tab_id).unwrap();
                        let session_id = format!("session-{tab_id}");
                        let binding_nonce = format!("nonce-{tab_id}");
                        tab.parent_tab_id = Some(parent_tab_id);
                        tab.agent_type = Some(agent.clone());
                        tab.agent_booting = true;
                        tab.agent_session_id = Some(session_id.clone());
                        tab.session_binding_nonce = Some(binding_nonce.clone());
                        tab.reported_session_binding_nonce = Some(binding_nonce.clone());
                        tab.task_binding = Some(crate::multiplexer::types::TaskBinding {
                            plan_id: Some(plan_id),
                            task_id: Some(task_id),
                            spawn_handle: None,
                            bound_at: 7,
                        });
                    }
                    spawned_tabs.lock().unwrap().push(tab_id.clone());
                    ready_senders
                        .lock()
                        .unwrap()
                        .insert(tab_id.clone(), ready_tx.unwrap());
                    Ok(crate::service::SpawnResult {
                        tab_id,
                        pty_id: String::new(),
                        agent,
                        model: None,
                        planned_model: None,
                        requested_model: None,
                        provider_resolved_model: None,
                        actual_model: None,
                        reasoning_effort: None,
                        reasoning_effort_provenance: Default::default(),
                        title,
                        description,
                        role,
                        role_profile,
                        warning: None,
                        model_fallback: None,
                        budget_notice: None,
                    })
                },
                move || {
                    observations.wait();
                },
            )
        })
    });

    let outcomes = starts.map(|start| start.join().unwrap());
    let mut winner = None;
    let mut refusal = None;
    for outcome in outcomes {
        match outcome {
            Ok(response) => {
                assert!(winner.replace(response).is_none());
            }
            Err(error) => {
                assert_eq!(error.code, crate::service::plan::RPC_DISPATCH_IN_FLIGHT);
                assert_eq!(error.data.as_ref().unwrap()["type"], "dispatch_in_flight");
                assert!(refusal.replace(error).is_none());
            }
        }
    }
    let winner = winner.expect("exactly one production dispatch succeeds");
    let refusal = refusal.expect("the competing production dispatch is refused");
    let winner_attempt = winner["attempt_id"].as_str().unwrap().to_string();
    let winner_tab = winner["tab_id"].as_str().unwrap().to_string();
    assert!(refusal.message.contains(&winner_attempt));
    assert!(refusal.message.contains(&winner_tab));

    let tabs = spawned_tabs.lock().unwrap().clone();
    assert_eq!(tabs, vec![winner_tab.clone()]);
    assert!(fixture.service.mux.read().get_tab(&winner_tab).is_some());
    assert_eq!(
        fixture.service.mux.read().snapshot().tabs.len(),
        tabs_before + 1,
        "the refused dispatch must not spawn or leave its reserved tab"
    );
    let task = fixture.service.plan_store.show_task(&task_id).unwrap().task;
    assert_eq!(task.assigned_tab_id.as_deref(), Some(winner_tab.as_str()));
    assert_eq!(task.execution_attempts.len(), 1);
    assert_eq!(task.execution_attempts[0].id, winner_attempt);
    assert_eq!(task.execution_attempts[0].closed_at, None);

    let session = fixture
        .service
        .mux
        .read()
        .get_tab(&winner_tab)
        .map(|tab| {
            (
                tab.agent_session_id.clone().unwrap(),
                tab.session_binding_nonce.clone().unwrap(),
            )
        })
        .unwrap();
    fixture
        .service
        .plan_store
        .finish_task_dispatch(
            &plan_id,
            &task_id,
            &winner_tab,
            &winner_attempt,
            &session.0,
            &session.1,
            task.lane.clone(),
            8,
        )
        .unwrap();
    fixture
        .service
        .plan_store
        .acknowledge_task_attempt(&plan_id, &task_id, &winner_tab, &winner_attempt, 9)
        .unwrap();
    ready_senders
        .lock()
        .unwrap()
        .remove(&winner_tab)
        .unwrap()
        .send(Ok(()))
        .unwrap();
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
/// retires the stale owner while applying the reconcile-blessed status update.
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn dead_owner_session_absent_tab_lets_bound_orchestrator_recover_update() {
    let (fixture, plan_id, task_id, _, _, owner) = update_fixture();
    bind_worker_owner(
        &fixture,
        &plan_id,
        &task_id,
        "dead-worker-tab",
        "worker-session",
        5,
    );
    let service = Arc::new(fixture.service.clone());

    let orch_caller = Some(serde_json::Value::Object(owner_params(&owner)));
    assert!(plan_owner_may_recover(
        "plan.task.update",
        &orch_caller,
        &service,
        &plan_id,
        Some("dead-worker-tab"),
        Some("worker-session"),
    ));

    let mut params = owner_params(&owner);
    params.insert("plan_id".into(), json!(plan_id.clone()));
    params.insert("task_id".into(), json!(task_id.clone()));
    params.insert("status".into(), json!("blocked"));
    params.insert("status_label".into(), json!("needs-reassign"));
    let params = Some(serde_json::Value::Object(params));
    verify_owner_epoch_for_task_mutation("plan.task.update", &params, &service).unwrap();

    let after = recovered_task(&fixture, &plan_id, &task_id);
    assert!(after.interruption.owner_tab_id.is_none());
    assert!(after.interruption.owner_session_id.is_none());
    assert!(after.interruption.owner_epoch > 5);
    assert!(after.assigned_tab_id.is_none());
}

/// Tab lingers in the mux but rebound to a NEW session: still a gone owner, so
/// the bound Orchestrator recovers exactly as with an absent tab.
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn lingering_tab_with_rebound_session_is_a_gone_owner() {
    let (fixture, plan_id, task_id, _, _, owner) = update_fixture();
    let rebound_tab = spawn_rebound_tab(&fixture, "new-rebound-session");
    bind_worker_owner(
        &fixture,
        &plan_id,
        &task_id,
        &rebound_tab,
        "worker-session",
        7,
    );
    let service = Arc::new(fixture.service.clone());

    let orch_caller = Some(serde_json::Value::Object(owner_params(&owner)));
    assert!(
        plan_owner_may_recover(
            "plan.task.update",
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
    params.insert("status".into(), json!("blocked"));
    params.insert("status_label".into(), json!("needs-reassign"));
    let params = Some(serde_json::Value::Object(params));
    verify_owner_epoch_for_task_mutation("plan.task.update", &params, &service).unwrap();

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
    bind_worker_owner(
        &fixture,
        &plan_id,
        &task_id,
        &live_worker,
        "worker-session",
        4,
    );
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

    let foreign_tab = spawn_rebound_tab(&fixture, "foreign-session");
    let foreign = Some(json!({
        "plan_id": plan_id,
        "task_id": task_id,
        "caller_tab_id": foreign_tab,
        "session_id": "foreign-session",
        "session_binding_nonce": "rebound-nonce",
        "owner_epoch": 4
    }));
    assert!(
        verify_owner_epoch_for_task_mutation("plan.task.update", &foreign, &service).is_err(),
        "the epoch guard must still fence a foreign caller off a live owner"
    );

    let after = recovered_task(&fixture, &plan_id, &task_id);
    assert_eq!(
        after.interruption.owner_epoch, 4,
        "the live owner's epoch is untouched"
    );
    assert_eq!(
        after.interruption.owner_tab_id.as_deref(),
        Some(live_worker.as_str())
    );
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn bound_orchestrator_closes_a_live_worker_task_without_matching_its_epoch() {
    let (fixture, plan_id, task_id, _, _, owner) = update_fixture();
    let live_worker = spawn_rebound_tab(&fixture, "worker-session");
    fixture
        .service
        .plan_store
        .assign_task(&plan_id, &task_id, &live_worker, None, 5)
        .unwrap();
    bind_worker_owner(
        &fixture,
        &plan_id,
        &task_id,
        &live_worker,
        "worker-session",
        9,
    );
    let service = Arc::new(fixture.service.clone());

    let mut params = owner_params(&owner);
    params.insert("plan_id".into(), json!(plan_id.clone()));
    params.insert("task_id".into(), json!(task_id.clone()));
    params.insert("owner_epoch".into(), json!(1));
    params.insert("result".into(), json!("accepted by orchestrator"));
    let params = Some(serde_json::Value::Object(params));

    verify_owner_epoch_for_task_mutation("plan.task.close", &params, &service).unwrap();
    super::super::plan_lifecycle::dispatch(
        "plan.task.close",
        &params,
        &fixture.service.app_handle,
        &service,
    )
    .unwrap();

    let task = recovered_task(&fixture, &plan_id, &task_id);
    assert_eq!(task.status, TaskStatus::Done);
    assert!(task.assigned_tab_id.is_none());
    assert!(task
        .execution_attempts
        .iter()
        .all(|attempt| attempt.closed_at.is_some()));
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn non_orchestrator_cannot_use_the_close_override() {
    let (fixture, plan_id, task_id, _, _, _) = update_fixture();
    let live_worker = spawn_rebound_tab(&fixture, "worker-session");
    bind_worker_owner(
        &fixture,
        &plan_id,
        &task_id,
        &live_worker,
        "worker-session",
        9,
    );
    let foreign_tab = spawn_rebound_tab(&fixture, "foreign-session");
    let service = Arc::new(fixture.service.clone());
    let params = Some(json!({
        "plan_id": plan_id,
        "task_id": task_id,
        "caller_tab_id": foreign_tab,
        "session_id": "foreign-session",
        "session_binding_nonce": "rebound-nonce",
        "owner_epoch": 1
    }));

    assert!(verify_owner_epoch_for_task_mutation("plan.task.close", &params, &service).is_err());
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
