use super::*;
use crate::api::plan_routes::tasks::tests::{owner_headers, update_fixture};
use crate::plan::{Task, TaskStatus};
use crate::service::test_fixture::ServiceFixture;
use std::sync::Arc;

fn stored_task(fixture: &ServiceFixture, plan_id: &str, task_id: &str) -> Task {
    fixture
        .service
        .plan_store
        .get(plan_id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap()
}

fn pause_body(reason: &str) -> TaskPauseBody {
    TaskPauseBody {
        reason: reason.into(),
        from: None,
    }
}

fn live_agent_tab(fixture: &ServiceFixture, session_id: &str, nonce: &str) -> String {
    let mut mux = fixture.service.mux.write();
    let workspace = mux.list_workspaces()[0].id.clone();
    let tab_id = mux.create_tab(&workspace).unwrap();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_session_id = Some(session_id.into());
    tab.session_binding_nonce = Some(nonce.into());
    tab.reported_session_binding_nonce = Some(nonce.into());
    tab_id
}

/// A dispatched task: non-zero owner epoch held by a live Worker session.
fn bind_worker_owner(
    fixture: &ServiceFixture,
    plan_id: &str,
    task_id: &str,
    worker_tab_id: &str,
    status: TaskStatus,
) {
    let mut plan = fixture.service.plan_store.get(plan_id).unwrap();
    let task = plan
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    task.status = status;
    task.interruption.owner_epoch = 2;
    task.interruption.owner_tab_id = Some(worker_tab_id.to_string());
    task.interruption.owner_session_id = Some("worker-session".into());
    fixture.service.plan_store.save(&mut plan, 5).unwrap();
}

fn non_owner_callers(stranger_tab_id: &str) -> Vec<HeaderMap> {
    let mut stranger = HeaderMap::new();
    stranger.insert("X-CodeTerm-Tab-Id", stranger_tab_id.parse().unwrap());
    stranger.insert("X-CodeTerm-Session-Id", "stranger-session".parse().unwrap());
    stranger.insert(
        "X-CodeTerm-Session-Binding-Nonce",
        "stranger-nonce".parse().unwrap(),
    );
    stranger.insert(
        "X-CodeTerm-Binding-Nonce",
        "stranger-nonce".parse().unwrap(),
    );
    stranger.insert("X-CodeTerm-Owner-Epoch", "2".parse().unwrap());
    vec![HeaderMap::new(), stranger]
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn rest_task_pause_and_resume_change_the_task_state() {
    let (fixture, plan_id, task_id, _, _, owner) = update_fixture();
    let service = Arc::new(fixture.service.clone());
    let headers = owner_headers(&owner);

    let Json(paused) = task_pause_core(
        &service,
        &headers,
        &plan_id,
        &task_id,
        pause_body("  waiting for quota  "),
    )
    .unwrap();
    assert_eq!(paused["ok"], true);
    assert_eq!(paused["task_id"], task_id.as_str());
    let task = stored_task(&fixture, &plan_id, &task_id);
    assert_eq!(task.status, TaskStatus::Paused);
    assert_eq!(task.pause_reason.as_deref(), Some("waiting for quota"));

    let Json(resumed) = task_resume_core(&service, &headers, &plan_id, &task_id).unwrap();
    assert_eq!(resumed["ok"], true);
    let task = stored_task(&fixture, &plan_id, &task_id);
    assert_eq!(task.status, TaskStatus::Pending);
    assert_eq!(task.pause_reason, None);
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn rest_task_pause_requires_a_reason() {
    let (fixture, plan_id, task_id, _, _, owner) = update_fixture();
    let service = Arc::new(fixture.service.clone());
    for reason in ["", "   "] {
        let (status, Json(body)) = task_pause_core(
            &service,
            &owner_headers(&owner),
            &plan_id,
            &task_id,
            pause_body(reason),
        )
        .unwrap_err();
        assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(body["plan_error"]["type"], "task_pause_requires_reason");
        assert_eq!(
            stored_task(&fixture, &plan_id, &task_id).status,
            TaskStatus::Pending
        );
    }
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn rest_task_pause_and_resume_refuse_a_caller_that_does_not_own_the_task() {
    let (fixture, plan_id, task_id, _, _, _) = update_fixture();
    let service = Arc::new(fixture.service.clone());
    let worker = live_agent_tab(&fixture, "worker-session", "worker-nonce");
    let stranger = live_agent_tab(&fixture, "stranger-session", "stranger-nonce");

    bind_worker_owner(
        &fixture,
        &plan_id,
        &task_id,
        &worker,
        TaskStatus::InProgress,
    );
    for headers in non_owner_callers(&stranger) {
        let refused = task_pause_core(
            &service,
            &headers,
            &plan_id,
            &task_id,
            pause_body("not my task"),
        );
        assert!(refused.is_err(), "a non-owner pause must be refused");
        assert_eq!(
            stored_task(&fixture, &plan_id, &task_id).status,
            TaskStatus::InProgress
        );
    }

    bind_worker_owner(&fixture, &plan_id, &task_id, &worker, TaskStatus::Paused);
    for headers in non_owner_callers(&stranger) {
        let refused = task_resume_core(&service, &headers, &plan_id, &task_id);
        assert!(refused.is_err(), "a non-owner resume must be refused");
        assert_eq!(
            stored_task(&fixture, &plan_id, &task_id).status,
            TaskStatus::Paused
        );
    }
}
