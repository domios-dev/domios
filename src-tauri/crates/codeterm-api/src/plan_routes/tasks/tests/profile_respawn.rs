use super::*;
use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
use codeterm_service_core::plan::{ExecutionIntent, TaskExecutionIntent, TaskStatus};

#[tokio::test]
async fn rest_respawn_uses_source_intent_for_launch_params_and_lane_preflight() {
    let (fixture, plan, task, _, _, owner) = update_fixture();
    let service = &fixture.service;
    let store = &service.plan_store;
    let provider = "rest-respawn-intent-fixture";
    service.agent_registry.register_provider(Box::new(
        codeterm_service_core::agent::test_provider::MockProvider::new(provider),
    ));
    store
        .assign_task(&plan, &task, "source-worker", None, 5)
        .unwrap();
    let mut approved = store.get(&plan).unwrap();
    approved.mode = codeterm_service_core::plan::PlanMode::Approved;
    approved.planning_state = codeterm_service_core::plan::PlanningState::Approved;
    store.save(&mut approved, 6).unwrap();
    let source = store.list_task_attempts(&plan, &task).unwrap()[0]
        .attempt_id
        .clone();
    let tabs = service.mux.read().snapshot().tabs.len();

    for (task_default, source_intent, refusal) in [
        (
            ExecutionIntent::Mutating,
            ExecutionIntent::ReadOnly,
            "respawn source tab is absent without confirmed executor termination",
        ),
        (
            ExecutionIntent::ReadOnly,
            ExecutionIntent::Mutating,
            "respawn lane verification failed",
        ),
    ] {
        store
            .edit_plan_fixture(&plan, 7, |plan| {
                let task = plan.tasks.iter_mut().find(|item| item.id == task).unwrap();
                task.status = TaskStatus::Blocked;
                task.status_label = Some("awaiting dependency evidence".into());
                task.execution_intent = TaskExecutionIntent::RepositoryMutation;
                task.interruption.execution_intent = task_default;
                task.execution_attempts[0].execution_intent = Some(source_intent.clone());
                Ok(())
            })
            .unwrap();
        let request = || TaskRespawnRequest {
            attempt_id: source.clone(),
            provider: Some(provider.into()),
            ..Default::default()
        };
        let before = full_task_record(&fixture, &plan, &task);
        let draft = store
            .build_task_respawn_draft(&plan, &task, request())
            .unwrap();
        let params = super::super::respawn::respawn_spawn_params(&draft, "Fixture", None);
        assert_eq!(params.execution_intent, Some(source_intent));
        let (status, Json(body)) = task_respawn(
            AxumState(crate::test_support::app_state(&fixture)),
            crate::auth::ActorHeaders::for_owner_test(owner_headers(&owner)),
            Path((plan.clone(), task.clone())),
            Json(request()),
        )
        .await
        .unwrap_err();
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(body.to_string().contains(refusal), "{body}");
        assert_eq!(full_task_record(&fixture, &plan, &task), before);
        assert_eq!(service.mux.read().snapshot().tabs.len(), tabs);
    }
}

#[tokio::test]
async fn rest_respawn_profile_refusal_is_typed_and_leaves_no_reservation_attempt_or_tab() {
    let (fixture, plan, task, _, _, owner) = update_fixture();
    let provider = "rest-respawn-profile-fixture";
    let manifest = PluginManifest::from_json_str(&json!({
        "id": provider, "version": "1.0.0", "displayName": "Fixture",
        "capabilities": {"agentProvider": true,
            "chatBackend": {"spawnable": true, "sessionConfig": true, "agentTasks": true},
            "agentRuntime": {"runtime": "stdio-jsonrpc", "command": {"binary": "fixture", "args": []},
                "launchProfiles": {"selected": {"binary": "fixture", "args": [], "ownerConfigured": true}},
                "requestMethods": {"initialize": ["initialize"]}, "notificationMethods": []}},
        "permissions": {"subprocess": {"allow": ["fixture"]}}
    }).to_string()).unwrap();
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(JsProvider::new(
            manifest,
            Arc::new(PluginVm::from_source(provider, "module.exports = {};".into()).unwrap()),
        )));
    fixture
        .service
        .plan_store
        .assign_task(&plan, &task, "source-worker", None, 5)
        .unwrap();
    let mut approved = fixture.service.plan_store.get(&plan).unwrap();
    approved.mode = codeterm_service_core::plan::PlanMode::Approved;
    approved.planning_state = codeterm_service_core::plan::PlanningState::Approved;
    fixture.service.plan_store.save(&mut approved, 6).unwrap();
    let source = fixture
        .service
        .plan_store
        .list_task_attempts(&plan, &task)
        .unwrap()[0]
        .attempt_id
        .clone();
    let before = full_task_record(&fixture, &plan, &task);
    let tabs = fixture.service.mux.read().snapshot().tabs.len();
    for (profile, code) in [
        ("unknown", "unknown_launch_profile"),
        ("selected", "launch_profile_not_installed"),
    ] {
        let (status, Json(body)) = task_respawn(
            AxumState(crate::test_support::app_state(&fixture)),
            crate::auth::ActorHeaders::for_owner_test(owner_headers(&owner)),
            Path((plan.clone(), task.clone())),
            Json(TaskRespawnRequest {
                attempt_id: source.clone(),
                provider: Some(provider.into()),
                launch_profile: Some(profile.into()),
                ..Default::default()
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(body["error"].is_object());
        assert_eq!(body["error"]["code"], code);
        assert_eq!(body["error"]["requested_profile"], profile);
        assert_eq!(body["error"]["declared_profile_ids"], json!(["selected"]));
        assert_eq!(full_task_record(&fixture, &plan, &task), before);
        assert_eq!(fixture.service.mux.read().snapshot().tabs.len(), tabs);
        let reservation = fixture
            .service
            .plan_store
            .reserve_task_respawn_startup(&plan, &task, &source, "probe-reservation")
            .unwrap();
        drop(reservation);
    }
    let sandbox_provider = "rest-respawn-sandbox-fixture";
    fixture.service.agent_registry.register_provider(Box::new(
        codeterm_service_core::agent::test_provider::MockProvider::new(sandbox_provider),
    ));
    let mut draft = fixture
        .service
        .plan_store
        .build_task_respawn_draft(
            &plan,
            &task,
            TaskRespawnRequest {
                attempt_id: source.clone(),
                provider: Some(sandbox_provider.into()),
                ..Default::default()
            },
        )
        .unwrap();
    draft.parent_tab_id = Some(owner.clone());
    let mut params = super::super::respawn::respawn_spawn_params(&draft, "Fixture", None);
    params.sandbox = Some("read-only".into());
    let state = crate::test_support::app_state(&fixture);
    let refusal = crate::agent_routes::spawn_rejection::validate_spawn_options(
        &state.svc,
        &draft.provider,
        &params,
    )
    .await
    .unwrap_err();
    assert_eq!(refusal.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        refusal.body["error"]["code"],
        "execution_sandbox_unsupported"
    );
    assert_eq!(refusal.body["error"]["provider"], sandbox_provider);
    assert_eq!(full_task_record(&fixture, &plan, &task), before);
    assert_eq!(fixture.service.mux.read().snapshot().tabs.len(), tabs);
    let reservation = fixture
        .service
        .plan_store
        .reserve_task_respawn_startup(&plan, &task, &source, "sandbox-probe-reservation")
        .unwrap();
    drop(reservation);
}

#[tokio::test]
async fn rest_respawn_unknown_or_manager_role_leaves_source_and_tabs_unchanged() {
    let (fixture, plan, task, _, _, owner) = update_fixture();
    fixture
        .service
        .plan_store
        .assign_task(&plan, &task, "source-worker", None, 5)
        .unwrap();
    let mut approved = fixture.service.plan_store.get(&plan).unwrap();
    approved.mode = codeterm_service_core::plan::PlanMode::Approved;
    approved.planning_state = codeterm_service_core::plan::PlanningState::Approved;
    fixture.service.plan_store.save(&mut approved, 6).unwrap();
    let source = fixture
        .service
        .plan_store
        .list_task_attempts(&plan, &task)
        .unwrap()[0]
        .attempt_id
        .clone();
    let before = full_task_record(&fixture, &plan, &task);
    let tabs = fixture.service.mux.read().snapshot().tabs.len();
    for role in ["unknown_worker", "planner"] {
        let (status, _) = task_respawn(
            AxumState(crate::test_support::app_state(&fixture)),
            crate::auth::ActorHeaders::for_owner_test(owner_headers(&owner)),
            Path((plan.clone(), task.clone())),
            Json(TaskRespawnRequest {
                attempt_id: source.clone(),
                role: Some(role.into()),
                ..Default::default()
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(full_task_record(&fixture, &plan, &task), before);
        assert_eq!(fixture.service.mux.read().snapshot().tabs.len(), tabs);
        drop(
            fixture
                .service
                .plan_store
                .reserve_task_respawn_startup(&plan, &task, &source, "probe")
                .unwrap(),
        );
    }
}
