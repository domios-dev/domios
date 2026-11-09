use super::*;
use crate::plan::OrchestratorRef;
use crate::service_plan::store_db::hot_write::tests::fixtures;
use crate::service_plan::{load_audit, PlanStore};
use std::sync::{Arc, Barrier};

fn goal() -> Goal {
    Goal {
        vector: "ship".into(),
        vision: "vision".into(),
        constraints: vec!["bounded".into()],
        postulates: vec!["atomic".into()],
    }
}

#[test]
fn goal_rows_and_bytes_ignore_unrelated_history_and_body_growth() {
    let mut prior = None;
    for (strides, attempts, body_bytes) in [(0, 0, 0), (2048, 0, 0), (0, 2048, 0), (0, 0, 32768)] {
        let root = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(root.path(), strides, attempts);
        plan.goal = Some(goal());
        plan.tasks[1].body.push_str(&"u".repeat(body_bytes));
        let actual_bytes = serde_json::to_vec(&plan).unwrap().len();
        let database = fixtures::database(root.path(), &plan);
        crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&database);
        drop(database);
        let store = PlanStore::open(root.path().join("plans.db")).unwrap();
        fixtures::audit(&store.database);
        load_audit::start(&store);
        let updated = store
            .patch_goal(
                &plan.id,
                GoalPatch {
                    vector: Some("next".into()),
                    ..Default::default()
                },
                20,
            )
            .unwrap();
        let work = super::super::take_work();
        let response_bytes = serde_json::to_vec(&updated).unwrap().len();
        println!("fixture={} tasks={} base_content_bytes={} actual_bytes={actual_bytes} operation=goal_patch strides={strides} unrelated_attempts={attempts} unrelated_body_bytes={body_bytes} changed_rows={:?} decoded_bytes={} serialized_bytes={} projected_text_bytes={} response_bytes={response_bytes} whole_plan_loads={:?}", fixtures::SEED, fixtures::TASKS, fixtures::CONTENT_BYTES, fixtures::audit_counts(&store.database), work[5], work[6], work[7], store.plan_loads_for_test());
        assert_eq!(&work[..5], &[0, 2, 0, 0, 0]);
        assert_eq!(
            fixtures::audit_counts(&store.database),
            vec![("plans".into(), 2)]
        );
        assert!(store.plan_loads_for_test().is_empty());
        assert!(work[5] < 1024 && work[6] < 1024);
        let measured = (work, response_bytes);
        if let Some(prior) = prior {
            assert_eq!(prior, measured);
        }
        prior = Some(measured);
        crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(
            &store.database,
        );
        drop(store);
        let reopened = PlanDatabase::open(root.path().join("plans.db")).unwrap();
        let stored = reopened.load_plan(&plan.id).unwrap().unwrap();
        assert_eq!(stored.goal, Some(updated));
        assert_eq!(stored.tasks, plan.tasks);
        assert_eq!(stored.strides, plan.strides);

        let baseline_root = tempfile::tempdir().unwrap();
        let baseline = fixtures::database(baseline_root.path(), &plan);
        crate::service_plan::store_sync::take_decoded_row_bytes();
        store_rows::take_serialized_row_bytes();
        let mut aggregate = baseline.load_plan(&plan.id).unwrap().unwrap();
        aggregate.goal.as_mut().unwrap().vector = "next".into();
        aggregate.updated_at = 20;
        let writes = baseline.persist_plan_rows(&aggregate, None).unwrap();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = store_rows::take_serialized_row_bytes();
        println!("fixture={} operation=goal_patch baseline=load_plan+persist_plan_rows strides={strides} unrelated_attempts={attempts} unrelated_body_bytes={body_bytes} decoded_bytes={decoded} serialized_bytes={serialized} writes={writes:?}", fixtures::SEED);
        assert!(decoded > 31 * 1024 * 1024 && serialized > 31 * 1024 * 1024);
    }
}

#[test]
fn goal_failure_rolls_back_payload_revision_and_commit_receipt() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = crate::plan::Plan::new(
        "goal".into(),
        root.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.goal = Some(goal());
    let database = fixtures::database(root.path(), &plan);
    database.pending_changes.lock().take(&plan.id);
    let before_revision =
        super::super::super::change_feed::revision(&database.connection.lock(), &plan.id).unwrap();
    database.connection.lock().execute_batch("CREATE TRIGGER reject_goal_revision BEFORE UPDATE ON plans WHEN json_extract(NEW.header_json,'$._revision') != coalesce(json_extract(OLD.header_json,'$._revision'),0) BEGIN SELECT RAISE(ABORT,'injected goal commit failure'); END;").unwrap();
    let error = database
        .change_goal(
            &plan.id,
            None,
            GoalPatch {
                vector: Some("rejected".into()),
                ..Default::default()
            },
            20,
        )
        .unwrap_err();
    assert!(error.to_string().contains("injected goal commit failure"));
    assert_eq!(database.load_plan(&plan.id).unwrap().unwrap(), plan);
    assert_eq!(
        super::super::super::change_feed::revision(&database.connection.lock(), &plan.id).unwrap(),
        before_revision
    );
    assert!(database.pending_changes.lock().take(&plan.id).is_none());
    let state: String = database
        .connection
        .lock()
        .query_row(
            "SELECT state FROM plan_operations ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, "failed");
    crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&database);
    drop(database);
    let reopened = PlanDatabase::open(root.path().join("plans.db")).unwrap();
    assert_eq!(reopened.load_plan(&plan.id).unwrap().unwrap(), plan);
}

#[test]
fn concurrent_goal_patches_preserve_independent_fields() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = crate::plan::Plan::new(
        "goal".into(),
        root.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.goal = Some(goal());
    drop(fixtures::database(root.path(), &plan));
    let store = Arc::new(PlanStore::open(root.path().join("plans.db")).unwrap());
    let barrier = Arc::new(Barrier::new(3));
    let handles: Vec<_> = [
        GoalPatch {
            vector: Some("next".into()),
            ..Default::default()
        },
        GoalPatch {
            vision: Some("new vision".into()),
            ..Default::default()
        },
    ]
    .into_iter()
    .map(|patch| {
        let store = store.clone();
        let barrier = barrier.clone();
        let id = plan.id.clone();
        std::thread::spawn(move || {
            barrier.wait();
            store.patch_goal(&id, patch, 20).unwrap();
        })
    })
    .collect();
    barrier.wait();
    for handle in handles {
        handle.join().unwrap();
    }
    let stored = store
        .database
        .load_plan(&plan.id)
        .unwrap()
        .unwrap()
        .goal
        .unwrap();
    assert_eq!(stored.vector, "next");
    assert_eq!(stored.vision, "new vision");
    assert_eq!(stored.constraints, goal().constraints);
    assert_eq!(stored.postulates, goal().postulates);
}
