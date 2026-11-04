use super::*;
use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};

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
}
