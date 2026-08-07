use super::*;
use crate::lock_hold::TimedRwLock;
use crate::multiplexer::state::MultiplexerState;
use crate::plan::task_update_fields::{valid_task_update_payloads, TASK_UPDATE_KEYS};
use crate::plan::{OrchestratorRef, Plan, TaskLaneMetadata};
use crate::service::plan::NewTask;
use crate::service::test_fixture::ServiceFixture;
use serde_json::json;
use std::sync::Arc;

/// The exact live owner identity the update seam authorizes against:
/// tab id, bound session and the nonce the tab itself reported back.
fn owner_headers(tab_id: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("X-CodeTerm-Tab-Id", tab_id.parse().unwrap());
    headers.insert("X-CodeTerm-Session-Id", "owner-session".parse().unwrap());
    headers.insert("X-CodeTerm-Binding-Nonce", "owner-nonce".parse().unwrap());
    headers
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
fn rest_update_rejects_unknown_keys_without_mutating_the_task() {
    for key in ["nonsense_field", "external_ref", "externalRefs"] {
        let (fixture, plan_id, task_id, _, _, owner) = update_fixture();
        let service = Arc::new(fixture.service.clone());
        let before = full_task_record(&fixture, &plan_id, &task_id);
        let mut body = json!({});
        body[key] = json!("ignored");

        let result = task_update_core(&service, &owner_headers(&owner), &plan_id, &task_id, body);
        let after = full_task_record(&fixture, &plan_id, &task_id);
        assert!(
            result.is_err(),
            "key {key} unexpectedly succeeded\nbefore:\n{before}\nafter:\n{after}"
        );
        let (status, Json(error)) = result.unwrap_err();

        assert_eq!(status, axum::http::StatusCode::BAD_REQUEST, "key: {key}");
        let message = error["error"].as_str().unwrap();
        assert!(message.contains(key), "{message}");
        assert!(message.contains("accepted"), "{message}");
        assert!(message.contains("external_refs"), "{message}");
        assert_eq!(
            after, before,
            "rejected key {key} changed the full task record"
        );
    }
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn rest_every_supported_update_key_reaches_the_real_handler() {
    let (fixture, plan_id, task_id, parent_task_id, dependency_task_id, owner) = update_fixture();
    let service = Arc::new(fixture.service.clone());
    let headers = owner_headers(&owner);
    let payloads = valid_task_update_payloads(&parent_task_id, &dependency_task_id);
    assert_eq!(
        payloads.iter().map(|(key, _)| *key).collect::<Vec<_>>(),
        TASK_UPDATE_KEYS
    );

    for (key, body) in payloads {
        let result = task_update_core(&service, &headers, &plan_id, &task_id, body);
        let failure = result
            .as_ref()
            .err()
            .map(|error| {
                let status = &error.0;
                let body = &error.1 .0;
                format!("{status}: {body}")
            })
            .unwrap_or_default();
        let Ok(Json(response)) = result else {
            panic!("REST failed supported key {key}: {failure}");
        };
        assert_eq!(response["ok"], true, "REST key {key}");
    }
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn rest_update_external_refs_round_trips_non_null() {
    let (fixture, plan_id, task_id, _, _, owner) = update_fixture();
    let service = Arc::new(fixture.service.clone());

    let Json(response) = task_update_core(
        &service,
        &owner_headers(&owner),
        &plan_id,
        &task_id,
        json!({ "external_refs": ["rollacode/codeterm-releases#17"] }),
    )
    .unwrap();
    assert_eq!(response["ok"], true);

    let plan = fixture.service.plan_store.get(&plan_id).unwrap();
    let task = plan.tasks.iter().find(|task| task.id == task_id).unwrap();
    assert_eq!(task.external_refs.len(), 1);
    assert_eq!(
        task.external_refs[0].canonical_url,
        "https://github.com/rollacode/codeterm-releases/issues/17"
    );
}

#[test]
fn rest_respawn_persists_spawned_attempt_title_and_session() {
    let dir = tempfile::tempdir().unwrap();
    let store = crate::service::plan::PlanStore::new_for_test(dir.path());
    let (mux_state, _) = MultiplexerState::new();
    let mux = Arc::new(TimedRwLock::new("mux", mux_state));
    let mut plan = Plan::new(
        "Respawn".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "task".into(),
                kind: "code".into(),
                lane: TaskLaneMetadata {
                    workspace_id: Some("ws-rest".into()),
                    ..Default::default()
                },
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let source_draft = crate::service::plan::task_attempts::TaskRespawnDraft {
        plan_id: plan.id.clone(),
        task_id: task_id.clone(),
        source_attempt: Default::default(),
        provider: "test-provider".into(),
        planned_model: None,
        model: None,
        parent_tab_id: None,
        workspace_id: Some("ws-rest".into()),
        title: None,
        explicit_title: false,
        cwd: dir.path().display().to_string(),
        task_prompt: "prompt".into(),
        report_command: "report".into(),
        plan_skill_manifest: vec![],
    };
    let created = store
        .record_task_respawn_attempt(
            &plan.id,
            &task_id,
            "source-tab",
            &source_draft,
            Some("worker"),
            3,
        )
        .unwrap();
    let ws = mux
        .write()
        .create_workspace("REST".into(), Some(dir.path().display().to_string()))
        .id;
    let tab_id = mux.write().create_tab(&ws).unwrap();
    mux.write().get_tab_mut(&tab_id).unwrap().agent_session_id = Some("rest-session".into());
    let draft = store
        .build_task_respawn_draft(
            &plan.id,
            &task_id,
            crate::service::plan::task_attempts::TaskRespawnRequest {
                attempt_id: created.attempt.id.clone(),
                provider: None,
                model: None,
                parent_tab_id: None,
                workspace_id: Some("ws-rest".into()),
                title: Some("Exact User Title".into()),
            },
        )
        .unwrap();
    let persisted = record_respawn_attempt_and_persist_identity_for(
        &store,
        &mux,
        Some("worker"),
        &plan.id,
        &task_id,
        &tab_id,
        &draft,
        "Exact User Title",
        4,
    )
    .unwrap();
    let task = store.show_task(&task_id).unwrap();
    let attempt = task
        .task
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == persisted.attempt.id)
        .unwrap();
    assert_eq!(attempt.executor_title.as_deref(), Some("Exact User Title"));
    assert_eq!(attempt.session_id.as_deref(), Some("rest-session"));
}

// The ack-deadline hint is offered to an Orchestrator as a runnable recovery
// command, and `plan task respawn` refuses to parse without the source attempt.
#[test]
fn respawn_hint_carries_the_source_attempt_flag() {
    let hint = respawn_spawn_command("pl_1", "task_2", "attempt_3");
    assert!(
        hint.contains(&format!("{RESPAWN_ATTEMPT_FLAG} attempt_3")),
        "respawn hint omits the required attempt selector: {hint}"
    );
}
