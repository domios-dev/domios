use super::*;
use crate::plan::{AgentRef, TaskStatus};
use crate::service::plan::batch::BatchRequest;
use crate::service::plan::NewTask;
use crate::service::plan_authority::test_fixture::fixture;
use serde_json::json;

#[test]
fn owner_completes_each_open_status_with_a_persisted_result_and_retires_assignment() {
    for status in [
        TaskStatus::Pending,
        TaskStatus::Blocked,
        TaskStatus::InProgress,
    ] {
        for batch in [false, true] {
            let (fixture, plan_id, _, owner) = fixture();
            let svc = &fixture.service;
            let task_id = svc
                .plan_store
                .add_task(
                    &plan_id,
                    NewTask {
                        name: "opaque-task".into(),
                        ..Default::default()
                    },
                    2,
                )
                .unwrap();
            svc.plan_store
                .with_plan(&plan_id, 3, |plan| {
                    let task = &mut plan.tasks[0];
                    task.status = status.clone();
                    task.assigned_tab_id = Some("opaque-worker".into());
                    task.lane.tab_id = Some("opaque-worker".into());
                    Ok(())
                })
                .unwrap();
            let note = "opaque-result-95β";
            if batch {
                let body = json!({
                    "caller_tab_id": owner.tab_id,
                    "session_id": owner.session_id,
                    "session_binding_nonce": owner.binding_nonce,
                    "ops": [
                        {"op":"set_task_status", "client_id":"task", "task":task_id, "status":"done"},
                        {"op":"add_milestone", "task":"$task", "kind":"status_update", "text":note}
                    ]
                });
                svc.authorize_batch_completions(&plan_id, &body).unwrap();
                let request: BatchRequest = serde_json::from_value(body).unwrap();
                svc.plan_store.apply_batch(&plan_id, request, 4).unwrap();
            } else {
                svc.authorize_task_completion(&plan_id, &owner, Some(note))
                    .unwrap();
                svc.plan_store
                    .close_task(
                        &plan_id,
                        &task_id,
                        Some(note),
                        AgentRef {
                            tab_id: owner.tab_id,
                            session_id: owner.session_id,
                            title: None,
                        },
                        4,
                    )
                    .unwrap();
            }
            let task = svc.plan_store.show_task(&task_id).unwrap().task;
            assert_eq!(task.status, TaskStatus::Done);
            assert!(task.assigned_tab_id.is_none());
            assert!(task.lane.tab_id.is_none());
            assert_eq!(task.milestones.last().unwrap().text, note);
        }
    }
}

#[test]
fn completion_requires_live_owner_and_a_note_for_the_same_task() {
    let (fixture, plan_id, planner, owner) = fixture();
    let svc = &fixture.service;
    let task_id = svc
        .plan_store
        .add_task(
            &plan_id,
            NewTask {
                name: "opaque-task".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    for note in [None, Some(""), Some(" \r\n\t")] {
        assert!(svc
            .authorize_task_completion(&plan_id, &owner, note)
            .is_err());
    }
    let stale = AuthenticatedPlanCaller {
        binding_nonce: Some("foreign-nonce".into()),
        ..owner.clone()
    };
    for caller in [&planner, &stale, &AuthenticatedPlanCaller::default()] {
        assert!(svc
            .authorize_task_completion(&plan_id, caller, Some("result"))
            .is_err());
    }
    for (target, note) in [("$task", ""), ("other-task", "result")] {
        let body = json!({
            "caller_tab_id":owner.tab_id, "session_id":owner.session_id,
            "session_binding_nonce":owner.binding_nonce,
            "ops":[
                {"op":"set_task_status", "client_id":"task", "task":task_id, "status":"done"},
                {"op":"add_milestone", "task":target, "kind":"status_update", "text":note}
            ]
        });
        assert!(svc.authorize_batch_completions(&plan_id, &body).is_err());
    }
    assert_eq!(
        svc.plan_store.show_task(&task_id).unwrap().task.status,
        TaskStatus::Pending
    );
}

#[test]
fn worker_record_cannot_bypass_review_with_status_or_upsert_done() {
    let (fixture, plan_id, worker, _) = fixture();
    let svc = &fixture.service;
    let task_id = svc
        .plan_store
        .add_task(
            &plan_id,
            NewTask {
                name: "opaque-task".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    svc.mux
        .write()
        .get_tab_mut(worker.tab_id.as_deref().unwrap())
        .unwrap()
        .role = Some(crate::multiplexer::types::AgentRole::Worker);
    for op in [
        json!({"op":"set_task_status", "client_id":"task", "task":task_id, "status":"done"}),
        json!({"op":"upsert_task", "client_id":"task", "handle":task_id, "status":"done"}),
    ] {
        let body = json!({
            "caller_tab_id":worker.tab_id, "session_id":worker.session_id,
            "session_binding_nonce":worker.binding_nonce,
            "ops":[op, {"op":"add_milestone", "task":"$task", "kind":"status_update", "text":"result"}]
        });
        assert!(svc.authorize_batch_completions(&plan_id, &body).is_err());
    }
    assert_eq!(
        svc.plan_store.show_task(&task_id).unwrap().task.status,
        TaskStatus::Pending
    );
}
