use super::*;
use crate::plan::{LoopStatus, OrchestratorRef, Plan};
use crate::service_plan::store_db::hot_write::tests::fixtures;
use crate::service_plan::{load_audit, PlanStore};
use std::sync::{Arc, Barrier};

fn running() -> LoopSpec {
    LoopSpec {
        id: "loop-selected".into(),
        target: "target".into(),
        budget: 2,
        count: 0,
        stop_when: None,
        status: LoopStatus::Running,
        stagnation: 0,
        deviation: None,
        last_iteration_at: None,
    }
}

#[test]
fn loop_family_rows_and_bytes_ignore_unrelated_history_and_body_growth() {
    let mut prior = None;
    for (strides, attempts, body_bytes) in [(0, 0, 0), (2048, 0, 0), (0, 2048, 0), (0, 0, 32768)] {
        let root = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(root.path(), strides, attempts);
        plan.tasks[1].body.push_str(&"u".repeat(body_bytes));
        let actual_bytes = serde_json::to_vec(&plan).unwrap().len();
        let database = fixtures::database(root.path(), &plan);
        crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&database);
        drop(database);
        let store = PlanStore::open(root.path().join("plans.db")).unwrap();
        fixtures::audit(&store.database);
        load_audit::start(&store);
        let id = store.create_loop(&plan.id, "target", 2, None, 20).unwrap();
        let create = super::super::take_work();
        store
            .set_loop(&id, Some(10), Some(Some("stop")), 21)
            .unwrap();
        let set = super::super::take_work();
        let advanced = store.advance_loop(&id, false, 22).unwrap();
        let advance = super::super::take_work();
        let result = store
            .record_loop(&id, "iteration", false, Some("task_0000"), 23)
            .unwrap();
        let record = super::super::take_work();
        store.stop_loop(&id, 24).unwrap();
        let stop = super::super::take_work();
        let response_bytes = serde_json::to_vec(&result).unwrap().len()
            + serde_json::to_vec(&advanced).unwrap().len();
        let work = [create, set, advance, record, stop];
        println!("fixture={} tasks={} base_content_bytes={} actual_bytes={actual_bytes} operation=loop_family strides={strides} unrelated_attempts={attempts} unrelated_body_bytes={body_bytes} changed_rows={:?} work_task_header_attempt_milestone_stride_decoded_serialized_projected_hashed={work:?} response_bytes={response_bytes}", fixtures::SEED, fixtures::TASKS, fixtures::CONTENT_BYTES, fixtures::audit_counts(&store.database));
        assert!(store.plan_loads_for_test().is_empty());
        for entry in [create, set, advance, stop] {
            assert_eq!(&entry[..5], &[0, 2, 0, 0, 0]);
            assert!(entry[5] < 1024 && entry[6] < 1024);
        }
        assert_eq!((record[0], record[2], record[3], record[4]), (2, 0, 1, 0));
        assert!(record[5] < 32 * 1024 && record[6] < 32 * 1024);
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
        assert_eq!(stored.loops[0].count, 2);
        assert_eq!(stored.loops[0].status, LoopStatus::Done);
        assert_eq!(
            stored.tasks[0].milestones[0].loop_id.as_deref(),
            Some(id.as_str())
        );
        assert_eq!(stored.tasks[1], plan.tasks[1]);
        assert_eq!(stored.strides, plan.strides);
        let baseline_root = tempfile::tempdir().unwrap();
        let baseline = fixtures::database(baseline_root.path(), &plan);
        crate::service_plan::store_sync::take_decoded_row_bytes();
        store_rows::take_serialized_row_bytes();
        let mut aggregate = baseline.load_plan(&plan.id).unwrap().unwrap();
        aggregate.loops.push(running());
        aggregate.updated_at = 20;
        let writes = baseline.persist_plan_rows(&aggregate, None).unwrap();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = store_rows::take_serialized_row_bytes();
        println!("fixture={} operation=create_loop baseline=load_plan+persist_plan_rows strides={strides} unrelated_attempts={attempts} unrelated_body_bytes={body_bytes} decoded_bytes={decoded} serialized_bytes={serialized} writes={writes:?}", fixtures::SEED);
        assert!(decoded > 31 * 1024 * 1024 && serialized > 31 * 1024 * 1024);
    }
}

#[test]
fn loop_record_failure_rolls_back_loop_task_and_milestone() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "loop".into(),
        root.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.tasks.push(load_audit::task(
        "task",
        crate::plan::TaskStatus::Pending,
        None,
    ));
    plan.loops.push(running());
    let database = fixtures::database(root.path(), &plan);
    database.pending_changes.lock().take(&plan.id);
    database.connection.lock().execute_batch("CREATE TRIGGER reject_loop_revision BEFORE UPDATE ON plans WHEN json_extract(NEW.header_json,'$._revision') != coalesce(json_extract(OLD.header_json,'$._revision'),0) BEGIN SELECT RAISE(ABORT,'injected loop commit failure'); END;").unwrap();
    let error = database
        .change_loop(
            &plan.id,
            "loop-selected",
            LoopChange::Record {
                note: "must roll back".into(),
                stop: true,
                preferred_task_id: Some("task".into()),
                milestone_id: "milestone".into(),
            },
            20,
        )
        .unwrap_err();
    assert!(error.to_string().contains("injected loop commit failure"));
    assert!(database.pending_changes.lock().take(&plan.id).is_none());
    crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&database);
    drop(database);
    let reopened = PlanDatabase::open(root.path().join("plans.db")).unwrap();
    assert_eq!(reopened.load_plan(&plan.id).unwrap().unwrap(), plan);
}

#[test]
fn concurrent_loop_records_cannot_exceed_budget_or_append_a_rejected_record() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "loop".into(),
        root.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.tasks.push(load_audit::task(
        "task",
        crate::plan::TaskStatus::Pending,
        None,
    ));
    let mut spec = running();
    spec.budget = 1;
    plan.loops.push(spec);
    drop(fixtures::database(root.path(), &plan));
    let store = Arc::new(PlanStore::open(root.path().join("plans.db")).unwrap());
    let barrier = Arc::new(Barrier::new(3));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let store = store.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.record_loop("loop-selected", "iteration", false, Some("task"), 20)
            })
        })
        .collect();
    barrier.wait();
    let results: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(
                result,
                Err(PlanError::BudgetExhausted {
                    count: 1,
                    budget: 1
                })
            ))
            .count(),
        1
    );
    let stored = store.database.load_plan(&plan.id).unwrap().unwrap();
    assert_eq!(stored.loops[0].count, 1);
    assert_eq!(stored.loops[0].status, LoopStatus::Exhausted);
    assert_eq!(stored.tasks[0].milestones.len(), 1);
}
