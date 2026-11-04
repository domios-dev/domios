use super::*;
use crate::plan::{TaskExecutionAttempt, TaskStatus};
use crate::service_plan::store_db::hot_write::tests::fixtures;
use crate::service_plan::store_db::read_projections::take_metrics;

#[test]
fn checkout_projection_preserves_plan_admission_live_attempt_order_and_exact_task_rules() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(directory.path(), 0, 0);
    plan.tasks[0].files.edit = vec!["src/a.rs".into()];
    plan.tasks[0].execution_attempts[0].placement = WorkerPlacement::Shared;
    plan.tasks[1].execution_attempts.push(TaskExecutionAttempt {
        id: "read-only".into(),
        placement: WorkerPlacement::ReadOnly,
        ..Default::default()
    });
    plan.tasks[1].execution_attempts.push(TaskExecutionAttempt {
        id: "exclusive".into(),
        placement: WorkerPlacement::CanonicalWriter,
        ..Default::default()
    });
    plan.status = crate::plan::PlanStatus::Archived;
    let database = fixtures::database(directory.path(), &plan);
    let roots = database.checkout_roots().unwrap();
    let peers = database
        .checkout_peer_facts(&roots, &[plan.id.clone()])
        .unwrap();
    assert_eq!(peers.len(), 1);
    assert_eq!(
        peers[0]
            .tasks
            .iter()
            .map(|task| task.id.as_str())
            .collect::<Vec<_>>(),
        vec!["task_0000", "task_0001"]
    );
    assert_eq!(
        peers[0].tasks[1]
            .execution_attempts
            .iter()
            .map(|attempt| attempt.id.as_str())
            .collect::<Vec<_>>(),
        vec!["read-only", "exclusive"]
    );
    assert!(
        crate::service_plan::shared_checkout::validate_exclusive_checkout(
            &peers,
            &plan,
            "task_0000"
        )
        .unwrap_err()
        .contains("task_0001")
    );
    for exact in [false, true] {
        let task = database
            .task_checkout_projection(&plan.id, "task_0000", exact)
            .unwrap();
        assert!(task.live_shared);
        assert_eq!(task.files, plan.tasks[0].files);
    }
    assert!(database
        .task_checkout_projection(&plan.id, "FIXTURE-0000", false)
        .is_ok());
    assert!(matches!(
        database.task_checkout_projection(&plan.id, "fixture-0000", true),
        Err(PlanError::TaskNotFound)
    ));
    assert!(matches!(
        database.task_checkout_projection("missing", "task_0000", false),
        Err(PlanError::NotFound)
    ));
    plan.tasks[0].execution_attempts[0].retired_at = Some(2);
    plan.tasks[1].execution_attempts[1].closed_at = Some(2);
    database.persist_plan_rows(&plan, None).unwrap();
    assert!(database
        .checkout_peer_facts(&roots, &[plan.id.clone()])
        .unwrap()
        .is_empty());
    assert!(
        !database
            .task_checkout_projection(&plan.id, "task_0000", true)
            .unwrap()
            .live_shared
    );
    plan.project_path = "changed-root".into();
    plan.canonical_project_path = "\u{2003}".into();
    database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!(
        database
            .task_checkout_projection(&plan.id, "task_0000", true)
            .unwrap()
            .checkout,
        plan.project_path
    );
    assert!(database
        .checkout_peer_facts(&roots, &[plan.id.clone()])
        .is_err());
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.files',42) WHERE task_id='task_0000'",[]).unwrap();
    assert!(database
        .task_checkout_projection(&plan.id, "task_0000", true)
        .is_err());
}

fn fixture(consumer: &str) {
    let mut prior = None;
    for (strides, attempts, body, history) in [
        (0, 1, 0, 0),
        (2048, 1, 0, 0),
        (0, 2048, 0, 0),
        (0, 1, 32768, 0),
        (0, 1, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.tasks[0].status = TaskStatus::InProgress;
        plan.tasks[0].files.edit = vec!["src/a.rs".into()];
        plan.tasks[0].execution_attempts[0].placement = WorkerPlacement::Shared;
        plan.tasks[0].body.push_str(&"x".repeat(body));
        let original = plan.tasks[0].execution_attempts[0].clone();
        let old = (0..history)
            .map(|index| TaskExecutionAttempt {
                id: format!("closed-{index}"),
                closed_at: Some(2),
                ..original.clone()
            })
            .collect::<Vec<_>>();
        plan.tasks[0].execution_attempts.splice(0..0, old);
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let roots = database.checkout_roots().unwrap();
        let peers = database
            .checkout_peer_facts(&roots, &[plan.id.clone()])
            .unwrap();
        assert_eq!(peers.len(), 1);
        let task = match consumer {
            "extend_task_edit_files" => Some(
                database
                    .task_checkout_projection(&plan.id, "task_0000", true)
                    .unwrap(),
            ),
            "record_task_respawn_attempt" => Some(
                database
                    .task_checkout_projection(&plan.id, "fixture-0000", false)
                    .unwrap(),
            ),
            _ => None,
        };
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&(peers, task)).unwrap().len();
        assert_eq!(
            metrics.queries,
            if consumer == "checkout_peer_plans" {
                3
            } else {
                4
            }
        );
        assert!(metrics.materialized_bytes < 4096);
        assert!(decoded < 4096);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer={consumer} tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}

#[test]
fn checkout_peer_plans_fixture_selects_ownership_without_serialization_or_writes() {
    fixture("checkout_peer_plans");
}

#[test]
fn extend_task_edit_files_fixture_selects_authority_without_serialization_or_writes() {
    fixture("extend_task_edit_files");
}

#[test]
fn record_task_respawn_attempt_fixture_selects_shared_files_without_serialization_or_writes() {
    fixture("record_task_respawn_attempt");
}
