use super::*;
use crate::plan::AgentRef;
use crate::service_plan::interruption::TaskMutationAuthority;
use crate::service_plan::task_assignment::AssignmentRuntimeBinding;
use crate::service_plan::task_attempts::BoundWorkerReport;
use codeterm_types::agent::ReportStatus;

fn bound_report(authority: TaskMutationAuthority, message: &str) -> BoundWorkerReport {
    BoundWorkerReport {
        message: message.into(),
        status: Some(ReportStatus::InReview),
        sha: Some("abcdef123456".into()),
        authority: Some(authority),
        ..Default::default()
    }
}

fn real_ingest_fixture() -> (
    tempfile::TempDir,
    PlanStore,
    String,
    String,
    String,
    TaskMutationAuthority,
) {
    let (dir, store, plan_id) = setup();
    let task_id = add_ready_task(&store, &plan_id, "real report lifecycle", 2);
    let binding = AssignmentRuntimeBinding {
        session_id: "worker-session".into(),
        session_binding_nonce: "worker-nonce".into(),
    };
    store
        .assign_task_with_runtime_binding(
            &plan_id,
            &task_id,
            "worker",
            None,
            Some("worker"),
            Some(&binding),
            None,
            3,
        )
        .unwrap();
    let assigned = store.show_task(&task_id).unwrap().task;
    let attempt_id = assigned.execution_attempts[0].id.clone();
    store
        .acknowledge_task_attempt(&plan_id, &task_id, "worker", &attempt_id, 4)
        .unwrap();
    let authority = TaskMutationAuthority {
        tab_id: "worker".into(),
        session_id: "worker-session".into(),
        session_binding_nonce: "worker-nonce".into(),
        owner_epoch: assigned.interruption.owner_epoch,
    };
    assert!(store
        .ingest_bound_worker_report(
            &plan_id,
            &task_id,
            &AgentRef::from_tab("worker"),
            bound_report(authority.clone(), "first review report"),
            5,
        )
        .unwrap());
    (dir, store, plan_id, task_id, attempt_id, authority)
}

#[test]
fn review_bounce_fresh_real_report_starts_new_generation_and_fences_old_epoch() {
    let (_dir, store, plan_id, task_id, attempt_id, authority) = real_ingest_fixture();
    let first = store
        .bounce_review(
            &plan_id,
            &task_id,
            "first correction",
            true,
            false,
            AgentRef::from_tab("reviewer"),
            6,
        )
        .unwrap();
    let first_continuation = first.continuation.clone().unwrap();
    store
        .claim_review_continuation_delivery(
            &plan_id,
            &task_id,
            &attempt_id,
            first_continuation.epoch,
            7,
        )
        .unwrap();
    assert!(store
        .finish_review_continuation(
            &plan_id,
            &task_id,
            &attempt_id,
            first_continuation.epoch,
            true,
            8,
        )
        .unwrap());

    assert_eq!(
        store.show_task(&task_id).unwrap().task.status,
        crate::plan::TaskStatus::InProgress
    );
    assert!(store
        .ingest_bound_worker_report(
            &plan_id,
            &task_id,
            &AgentRef::from_tab("worker"),
            bound_report(authority.clone(), "fresh review report"),
            9,
        )
        .unwrap());
    let fresh_entry = store.show_task(&task_id).unwrap().task;
    let fresh_attempt = &fresh_entry.execution_attempts[0];
    assert_eq!(fresh_entry.status, crate::plan::TaskStatus::InReview);
    assert_eq!(fresh_entry.status_label, None);
    assert_eq!(
        fresh_attempt.continuation.epoch,
        first_continuation.epoch + 1
    );
    assert_eq!(fresh_attempt.continuation.state, None);
    assert_eq!(fresh_attempt.continuation.milestone_id, None);

    let second = store
        .bounce_review(
            &plan_id,
            &task_id,
            "second correction",
            true,
            false,
            AgentRef::from_tab("reviewer"),
            10,
        )
        .unwrap();
    let second_continuation = second.continuation.clone().unwrap();
    assert_eq!(second.review_round, first.review_round + 1);
    assert_ne!(second.milestone_id, first.milestone_id);
    assert!(second_continuation.epoch > first_continuation.epoch);
    assert_eq!(second_continuation.state, "awaiting_continuation");

    let replay = store
        .bounce_review(
            &plan_id,
            &task_id,
            "second correction",
            true,
            false,
            AgentRef::from_tab("reviewer"),
            11,
        )
        .unwrap();
    assert_eq!(replay, second);

    let stale_claim = store
        .claim_review_continuation_delivery(
            &plan_id,
            &task_id,
            &attempt_id,
            first_continuation.epoch,
            12,
        )
        .expect_err("the completed first epoch cannot claim the new bounce");
    assert!(matches!(
        stale_claim,
        PlanError::ReviewContinuationDeliveryRefused {
            actual_epoch: Some(epoch),
            ..
        } if epoch == second_continuation.epoch
    ));
    assert!(!store
        .finish_review_continuation(
            &plan_id,
            &task_id,
            &attempt_id,
            first_continuation.epoch,
            true,
            13,
        )
        .unwrap());

    store
        .claim_review_continuation_delivery(
            &plan_id,
            &task_id,
            &attempt_id,
            second_continuation.epoch,
            14,
        )
        .unwrap();
    assert!(store
        .finish_review_continuation(
            &plan_id,
            &task_id,
            &attempt_id,
            second_continuation.epoch,
            true,
            15,
        )
        .unwrap());
    assert_eq!(
        store.show_task(&task_id).unwrap().task.status,
        crate::plan::TaskStatus::InProgress
    );
    assert!(store
        .ingest_bound_worker_report(
            &plan_id,
            &task_id,
            &AgentRef::from_tab("worker"),
            bound_report(authority.clone(), "third review report"),
            16
        )
        .unwrap());
    let third = store
        .bounce_review(
            &plan_id,
            &task_id,
            "third correction",
            true,
            false,
            AgentRef::from_tab("reviewer"),
            17,
        )
        .unwrap();
    let third_continuation = third.continuation.unwrap();
    store
        .claim_review_continuation_delivery(
            &plan_id,
            &task_id,
            &attempt_id,
            third_continuation.epoch,
            18,
        )
        .unwrap();
    assert!(store
        .finish_review_continuation(
            &plan_id,
            &task_id,
            &attempt_id,
            third_continuation.epoch,
            false,
            19
        )
        .unwrap());
    let failed = store.show_task(&task_id).unwrap().task;
    assert_eq!(
        failed.status_label.as_deref(),
        Some("bounce_continuation_failed")
    );
    assert!(store
        .claim_review_continuation_delivery(
            &plan_id,
            &task_id,
            &attempt_id,
            third_continuation.epoch,
            20,
        )
        .is_err());
}
