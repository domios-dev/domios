mod profile_respawn;

use super::*;
use axum::http::HeaderMap;
use codeterm_service_core::lock_hold::TimedRwLock;
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::plan::task_update_fields::{
    valid_task_update_payloads, TASK_UPDATE_KEYS,
};
use codeterm_service_core::plan::{OrchestratorRef, Plan, TaskLaneMetadata};
use codeterm_service_core::service::plan::NewTask;
use codeterm_service_core::service::test_fixture::ServiceFixture;
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn task_mutation_authority_storage_failure_is_internal_before_any_write() {
    let (fixture, plan_id, task_id, _, _, owner) = update_fixture();
    let headers = crate::auth::ActorHeaders::for_owner_test(owner_headers(&owner));
    let before = full_task_record(&fixture, &plan_id, &task_id);
    let service = Arc::new(fixture.service.clone());
    let connection =
        rusqlite::Connection::open(fixture.temp.path().join(".domios/plans.db")).unwrap();
    connection
        .execute(
            "UPDATE plans SET header_json=json_set(header_json,'$.owner',42) WHERE id=?1",
            [&plan_id],
        )
        .unwrap();
    for kind in [
        TaskMutationKind::Ordinary,
        TaskMutationKind::Recovery,
        TaskMutationKind::BlockDecision,
    ] {
        let (status, Json(body)) =
            verify_task_mutation_owner(&service, &headers, &plan_id, &task_id, kind, None)
                .await
                .unwrap_err();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["type"], "plan_authority_store_read_failed");
    }
    connection
        .execute(
            "UPDATE plans SET header_json=json_set(header_json,'$.owner',json(?1)) WHERE id=?2",
            rusqlite::params![
                serde_json::to_string(&OrchestratorRef {
                    tab_id: Some(owner),
                    session_id: Some("owner-session".into()),
                    title: None
                })
                .unwrap(),
                plan_id
            ],
        )
        .unwrap();
    assert_eq!(full_task_record(&fixture, &plan_id, &task_id), before);
}

#[test]
fn task_board_page_rejects_incomplete_or_unsupported_selection() {
    let incomplete = TaskBoardPageQuery {
        after_created_at: Some(7),
        ..Default::default()
    };
    let (status, Json(body)) = task_board_cursor(&incomplete).unwrap_err();
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("created_at, position, and task_id"));
    let no_status = TaskBoardPageQuery {
        after_created_at: Some(7),
        after_position: Some(4),
        after_task_id: Some("task-4".into()),
        ..Default::default()
    };
    let (status, Json(body)) = task_board_cursor(&no_status).unwrap_err();
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("requires a status"));
    assert!(!task_board_status_valid(Some("not-a-status")));
    assert!(task_board_status_valid(Some("pending")));
    assert!(task_board_status_valid(None));
}

#[test]
fn task_board_page_accepts_the_complete_stable_cursor() {
    let query = TaskBoardPageQuery {
        after_created_at: Some(7),
        after_position: Some(4),
        after_task_id: Some("task-4".into()),
        status: Some("done".into()),
        ..Default::default()
    };
    assert_eq!(
        task_board_cursor(&query).unwrap(),
        Some((7, 4, "task-4".into()))
    );
    assert!(task_board_status_valid(query.status.as_deref()));
}

#[test]
fn task_board_page_preserves_team_binding_authorization_errors() {
    let (status, _) = task_board_team_plan_err(TeamPlanError::Team("not authorized".into()));
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
    let (status, _) = task_board_team_plan_err(TeamPlanError::Team("invalid binding".into()));
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

#[test]
fn committed_task_event_matches_scoped_snapshot_and_stale_team_binding_denies_read() {
    let (fixture, plan_id, task_id, parent_id, _, owner) = update_fixture();
    let svc = &fixture.service;
    svc.plan_store
        .edit_plan_fixture(&plan_id, 5, |plan| {
            plan.tasks
                .iter_mut()
                .find(|task| task.id == parent_id)
                .unwrap()
                .body = "unrelated body sentinel".into();
            Ok(())
        })
        .unwrap();
    svc.emit_committed_plan_changed(&plan_id);
    fixture.ui.0.lock().clear();
    let Json(updated) = task_update_core(
        &Arc::new(svc.clone()),
        &crate::auth::ActorHeaders::for_owner_test(owner_headers(&owner)),
        &plan_id,
        &task_id,
        json!({"body":"selected body"}),
    )
    .unwrap();
    assert_eq!(updated["ok"], true);
    let event = fixture
        .ui
        .0
        .lock()
        .iter()
        .find(|(event, _)| event == "plan-changed")
        .map(|(_, payload)| payload.clone())
        .expect("committed event");
    let Json(detail) = task_detail_core(svc, &plan_id, &task_id).unwrap();
    assert_eq!(event["plan_id"], plan_id);
    assert_eq!(event["task_ids"], json!([task_id]));
    assert_eq!(event["reset"], false);
    assert_eq!(event["revision"], detail["revision"]);
    let Json(board) =
        crate::plan_routes::projections::board_state_core(svc, &plan_id, 0, false).unwrap();
    assert_eq!(board["revision"], event["revision"]);
    assert!(board.get("recent_strides").is_none() && board.get("tasks").is_none());
    let kind = codeterm_service_core::service::plan::task_history::TaskHistoryKind::Milestones;
    let Json(history) = history::task_history_core(svc, &plan_id, &task_id, kind, None, 2).unwrap();
    assert_eq!(history["revision"], event["revision"]);
    assert!(history["items"].as_array().unwrap().len() <= 2);
    assert_eq!(detail["task"]["body"], "selected body");
    assert!(!serde_json::to_string(&event).unwrap().contains("body"));
    assert!(!serde_json::to_string(&detail)
        .unwrap()
        .contains("unrelated body sentinel"));
    svc.plan_store
        .edit_plan_fixture(&plan_id, 6, |plan| {
            plan.team_id = Some("missing-team".into());
            Ok(())
        })
        .unwrap();
    let (status, Json(error)) = task_detail_core(svc, &plan_id, &task_id).unwrap_err();
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(error.get("task").is_none());
    let (status, Json(board)) =
        crate::plan_routes::projections::board_state_core(svc, &plan_id, 0, false).unwrap_err();
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(board.get("mission_control").is_none());
    let (status, Json(error)) =
        history::task_history_core(svc, &plan_id, &task_id, kind, None, 2).unwrap_err();
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(error.get("items").is_none());
}

#[test]
fn task_detail_resolves_scoped_id_handle_and_key_with_exact_id_precedence() {
    let (fixture, plan_id, task_id, parent_id, dependency_id, _) = update_fixture();
    let svc = &fixture.service;
    svc.plan_store
        .edit_plan_fixture(&plan_id, 5, |plan| {
            let selected = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            selected.handle = Some("qa187-race".into());
            selected.key = Some("QA1-1103".into());
            let parent = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == parent_id)
                .unwrap();
            parent.handle = Some(task_id.clone());
            Ok(())
        })
        .unwrap();
    let mut other = Plan::new(
        "Other scope".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef::default(),
    );
    let mut foreign = svc.plan_store.try_get(&plan_id).unwrap().tasks[0].clone();
    foreign.id = "task_foreign".into();
    foreign.handle = Some("qa187-race".into());
    foreign.key = Some("QA1-1103".into());
    other.tasks = vec![foreign];
    svc.plan_store.save(&mut other, 6).unwrap();
    svc.plan_store.reset_plan_loads_for_test();
    let Json(expected) = task_detail_core(svc, &plan_id, &task_id).unwrap();
    assert_eq!(expected["task_id"], task_id);
    for reference in [&task_id, "qa187-race", "QA187-RACE", "QA1-1103", "qa1-1103"] {
        let Json(actual) = task_detail_core(svc, &plan_id, reference).unwrap();
        assert_eq!(actual, expected, "reference {reference}");
        assert_eq!(
            svc.plan_store
                .resolve_task_ref(Some(&plan_id), reference)
                .unwrap()
                .task_id,
            task_id
        );
    }
    for reference in ["task_foreign", "missing", "qa187-ra"] {
        let (status, Json(body)) = task_detail_core(svc, &plan_id, reference).unwrap_err();
        assert_eq!(status, StatusCode::NOT_FOUND, "{reference}: {body}");
        assert_eq!(body["plan_error"]["type"], "task_not_found");
    }
    assert!(svc.plan_store.plan_loads_for_test().is_empty());
    svc.plan_store
        .edit_plan_fixture(&plan_id, 7, |plan| {
            plan.tasks
                .iter_mut()
                .find(|task| task.id == dependency_id)
                .unwrap()
                .handle = Some("qa1-1103".into());
            Ok(())
        })
        .unwrap();
    for reference in ["QA1-1103", "qa1-1103"] {
        let (status, Json(body)) = task_detail_core(svc, &plan_id, reference).unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["error"].as_str().unwrap().contains("ambiguous"));
        assert!(svc
            .plan_store
            .resolve_task_ref(Some(&plan_id), reference)
            .is_err());
    }
    assert_eq!(
        task_detail_core(svc, &plan_id, &task_id).unwrap().0["task_id"],
        task_id
    );
}

/// The exact live owner identity the update seam authorizes against:
/// tab id, bound session and the nonce the tab itself reported back.
pub(crate) fn owner_headers(tab_id: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("X-CodeTerm-Tab-Id", tab_id.parse().unwrap());
    headers.insert("X-CodeTerm-Session-Id", "owner-session".parse().unwrap());
    headers.insert("X-CodeTerm-Binding-Nonce", "owner-nonce".parse().unwrap());
    headers
}

pub(crate) fn update_fixture() -> (ServiceFixture, String, String, String, String, String) {
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

#[test]
fn rest_update_rejects_unknown_keys_without_mutating_the_task() {
    for key in ["nonsense_field", "external_ref", "externalRefs"] {
        let (fixture, plan_id, task_id, _, _, owner) = update_fixture();
        let service = Arc::new(fixture.service.clone());
        let before = full_task_record(&fixture, &plan_id, &task_id);
        let mut body = json!({});
        body[key] = json!("ignored");

        let result = task_update_core(
            &service,
            &crate::auth::ActorHeaders::for_owner_test(owner_headers(&owner)),
            &plan_id,
            &task_id,
            body,
        );
        let after = full_task_record(&fixture, &plan_id, &task_id);
        assert!(
            result.is_err(),
            "key {key} unexpectedly succeeded\nbefore:\n{before}\nafter:\n{after}"
        );
        let (status, Json(error)) = result.unwrap_err();

        assert_eq!(status, axum::http::StatusCode::BAD_REQUEST, "key: {key}");
        let message = error["error"].as_str().unwrap();
        assert!(message.contains(key), "{message}");
        assert_eq!(
            after, before,
            "rejected key {key} changed the full task record"
        );
    }
}

#[test]
fn rest_every_supported_update_key_reaches_the_real_handler() {
    codeterm_service_core::ports::socket::register_test_stub();
    let (fixture, plan_id, task_id, parent_task_id, dependency_task_id, owner) = update_fixture();
    let service = Arc::new(fixture.service.clone());
    let headers = owner_headers(&owner);
    let payloads = valid_task_update_payloads(&parent_task_id, &dependency_task_id);
    assert_eq!(
        payloads.iter().map(|(key, _)| *key).collect::<Vec<_>>(),
        TASK_UPDATE_KEYS
    );

    for (key, body) in payloads {
        let result = task_update_core(
            &service,
            &crate::auth::ActorHeaders::for_owner_test(headers.clone()),
            &plan_id,
            &task_id,
            body,
        );
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

#[test]
fn rest_update_external_refs_round_trips_non_null() {
    let (fixture, plan_id, task_id, _, _, owner) = update_fixture();
    let service = Arc::new(fixture.service.clone());

    let Json(response) = task_update_core(
        &service,
        &crate::auth::ActorHeaders::for_owner_test(owner_headers(&owner)),
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
fn rest_cleanup_done_refusal_returns_conflict_for_unmerged_lane_branch() {
    let (fixture, plan_id, task_id, _, _, owner) = update_fixture();
    let project = fixture.temp.path();
    let worktree = project.join("lane/rest-cleanup-refusal");
    let branch = "lane/rest-cleanup-refusal";
    fixture
        .service
        .plan_store
        .update_task_lane(
            &plan_id,
            &task_id,
            TaskLaneUpdate {
                worktree_path: Some(Some(worktree.display().to_string())),
                branch: Some(Some(branch.into())),
                ..Default::default()
            },
            5,
        )
        .unwrap();

    let result = task_update_core_with_lane_cleanup_check(
        &Arc::new(fixture.service.clone()),
        &crate::auth::ActorHeaders::for_owner_test(owner_headers(&owner)),
        &plan_id,
        &task_id,
        json!({"lane": {"cleanup_status": "done"}}),
        |store, plan_id, task_id, lane| {
            let lane = lane.expect("cleanup status reaches the lane verifier");
            assert_eq!(lane.cleanup_status, Some(Some("done".to_owned())));
            let task = store
                .get(plan_id)
                .unwrap()
                .tasks
                .into_iter()
                .find(|task| task.id == task_id)
                .unwrap();
            assert_eq!(task.lane.branch.as_deref(), Some(branch));
            Err(
                codeterm_service_core::service::lane_removal::CleanupStatusDoneError::Refused(
                    format!(
                        "cleanup is not complete: branch '{branch}' still has commits not merged into target checkout"
                    ),
                ),
            )
        },
    );

    let (status, Json(body)) = result.expect_err("unmerged commits refuse cleanup completion");
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
    assert!(body["error"].as_str().unwrap().contains(branch));
}

#[test]
fn lifecycle_reassignment_rest_respawn_defers_unverified_identity() {
    let dir = tempfile::tempdir().unwrap();
    let store = codeterm_service_core::service::plan::PlanStore::new_for_test(dir.path());
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
    store
        .assign_task(&plan.id, &task_id, "source-tab", None, 3)
        .unwrap();
    let source_attempt_id = store.list_task_attempts(&plan.id, &task_id).unwrap()[0]
        .attempt_id
        .clone();
    let ws = mux
        .write()
        .create_workspace("REST".into(), Some(dir.path().display().to_string()))
        .id;
    let tab_id = mux.write().create_tab(&ws).unwrap();
    mux.write().get_tab_mut(&tab_id).unwrap().agent_session_id = Some("rest-session".into());
    let mut draft = store
        .build_task_respawn_draft(
            &plan.id,
            &task_id,
            codeterm_service_core::service::plan::task_attempts::TaskRespawnRequest {
                launch_profile: None,
                reasoning_effort: None,
                attempt_id: source_attempt_id,
                provider: None,
                model: None,
                parent_tab_id: None,
                workspace_id: Some("ws-rest".into()),
                title: Some("Exact User Title".into()),
            },
        )
        .unwrap();
    draft.provider = "codex".into();
    draft.model = Some("gpt-6-luna".into());
    draft.reasoning_effort = Some("xhigh".into());
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
    let task = store.inspect_task_for_test(&task_id).unwrap();
    let attempt = task
        .task
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == persisted.0.attempt.id)
        .unwrap();
    assert_eq!(attempt.executor_title.as_deref(), Some("Exact User Title"));
    assert_eq!(attempt.provider.as_deref(), Some("codex"));
    assert_eq!(attempt.model.as_deref(), Some("gpt-6-luna"));
    assert_eq!(attempt.reasoning_effort.as_deref(), Some("xhigh"));
    assert!(attempt.session_id.is_none());
    assert!(attempt.session_binding_nonce.is_none());
    assert!(attempt.acknowledged_at.is_none());
    assert_eq!(task.task.status_label.as_deref(), Some("starting"));
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
    assert!(
        !hint.contains("--execution-intent"),
        "intent is inherited: {hint}"
    );
    assert!(
        !hint.contains("--provider"),
        "provider is inherited: {hint}"
    );
    assert!(!hint.contains("--model"), "model is inherited: {hint}");
}
