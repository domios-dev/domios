use super::*;
use codeterm_service_core::plan::OrchestratorRef;
use codeterm_service_core::service::plan::task_queries::{TaskListEmptyReason, TaskSummary};

fn summary() -> TaskSummary {
    TaskSummary {
        plan_id: "pl_one".into(),
        plan_title: "One".into(),
        plan_owner: OrchestratorRef::default(),
        display_key: "ONE".into(),
        task: serde_json::from_value(json!({
            "id": "task_one",
            "name": "One",
            "type": "task",
            "model": null,
            "model_rationale": null,
            "test_plan": null,
            "correctness": null,
            "status": "pending",
            "status_label": null,
            "intent": null,
            "assigned_tab_id": null,
            "peer": null
        }))
        .expect("task fixture must deserialize"),
        section_titles: vec![],
    }
}

#[test]
fn list_response_carries_the_empty_reason_verbatim() {
    let reason = TaskListEmptyReason::AllTasksInactive { hidden: 3 };
    let Json(body) = list_response(TaskListResult {
        tasks: vec![],
        empty_reason: Some(reason.clone()),
    });

    assert_eq!(body["tasks"], json!([]));
    assert_eq!(
        body["empty_reason"],
        serde_json::to_value(&reason).expect("reason must serialize")
    );
}

#[test]
fn list_response_omits_the_empty_reason_key_for_a_non_empty_listing() {
    let Json(body) = list_response(TaskListResult {
        tasks: vec![summary()],
        empty_reason: None,
    });

    assert_eq!(body["tasks"].as_array().map(Vec::len), Some(1));
    assert!(body.get("empty_reason").is_none());
}

#[test]
fn team_active_plan_conflict_is_409_with_plan_error_json() {
    let (status, Json(body)) = plan_err(PlanError::TeamAlreadyHasActivePlan {
        team_id: "team_alpha".into(),
        existing_plan_id: "pl_existing".into(),
        existing_title: "Existing".into(),
    });
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["plan_error"]["type"], "team_already_has_active_plan");
}

#[test]
fn review_hold_required_is_409_with_plan_error_json() {
    let (status, Json(body)) = plan_err(PlanError::ReviewHoldRequired);
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["plan_error"]["type"], "review_hold_required");
}
