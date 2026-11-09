use super::*;
use crate::plan::{OrchestratorRef, Plan};
use crate::service_plan::store_db::hot_write::tests::fixtures;
use crate::service_plan::{load_audit, PlanStore};
use std::sync::{Arc, Barrier};

#[test]
fn gate_family_rows_and_bytes_ignore_unrelated_history_and_body_growth() {
    let mut prior = None;
    for (strides, attempts, body_bytes) in [(0, 0, 0), (2048, 0, 0), (0, 2048, 0), (0, 0, 32768)] {
        let root = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(root.path(), strides, attempts);
        plan.tasks[1].body.push_str(&"u".repeat(body_bytes));
        let database = fixtures::database(root.path(), &plan);
        crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&database);
        drop(database);
        let store = PlanStore::open(root.path().join("plans.db")).unwrap();
        fixtures::audit(&store.database);
        load_audit::start(&store);
        let (gate, request) = store
            .ask_gate_with_planner_request(
                &plan.id,
                "question",
                "context",
                &["task_0000".into()],
                Some("planner".into()),
                Some("owner"),
                Some("session"),
                20,
            )
            .unwrap();
        let ask_work = super::super::take_work();
        store
            .escalate_gate(&plan.id, &gate.id, Some("escalated"), 21)
            .unwrap();
        let escalate_work = super::super::take_work();
        let answered = store
            .answer_gate(&plan.id, &gate.id, "answer", Some("planner"), 22)
            .unwrap();
        let answer_work = super::super::take_work();
        let work = [ask_work, escalate_work, answer_work];
        let response_bytes = serde_json::to_vec(&(gate, request, &answered))
            .unwrap()
            .len();
        println!("fixture={} tasks={} base_content_bytes={} operation=gate_family strides={strides} unrelated_attempts={attempts} unrelated_body_bytes={body_bytes} changed_rows={:?} work_task_header_attempt_milestone_stride_decoded_serialized_projected_hashed={work:?} response_bytes={response_bytes}", fixtures::SEED, fixtures::TASKS, fixtures::CONTENT_BYTES, fixtures::audit_counts(&store.database));
        assert!(store.plan_loads_for_test().is_empty());
        assert_eq!(
            (ask_work[0], ask_work[2], ask_work[3], ask_work[4]),
            (2, 0, 1, 0)
        );
        assert_eq!(&escalate_work[..5], &[0, 2, 0, 0, 0]);
        assert_eq!(
            (
                answer_work[0],
                answer_work[2],
                answer_work[3],
                answer_work[4]
            ),
            (2, 0, 1, 0)
        );
        for entry in work {
            assert!(entry[5] < 32 * 1024 && entry[6] < 32 * 1024);
        }
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
        assert_eq!(stored.gates, vec![answered]);
        assert_eq!(stored.tasks[0].status, TaskStatus::Pending);
        assert_eq!(stored.tasks[0].milestones.len(), 2);
        assert_eq!(stored.tasks[1], plan.tasks[1]);
        assert_eq!(stored.strides, plan.strides);
        assert_eq!(
            (stored.planner_requests.len(), stored.planner_inbox.len()),
            (1, 1)
        );
        let baseline_root = tempfile::tempdir().unwrap();
        let baseline = fixtures::database(baseline_root.path(), &plan);
        crate::service_plan::store_sync::take_decoded_row_bytes();
        store_rows::take_serialized_row_bytes();
        let mut aggregate = baseline.load_plan(&plan.id).unwrap().unwrap();
        aggregate.tasks[0] = stored.tasks[0].clone();
        aggregate.gates = stored.gates;
        aggregate.planner_requests = stored.planner_requests;
        aggregate.planner_inbox = stored.planner_inbox;
        aggregate.updated_at = 22;
        let writes = baseline.persist_plan_rows(&aggregate, None).unwrap();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = store_rows::take_serialized_row_bytes();
        println!("fixture={} operation=gate_roundtrip baseline=load_plan+persist_plan_rows strides={strides} unrelated_attempts={attempts} unrelated_body_bytes={body_bytes} decoded_bytes={decoded} serialized_bytes={serialized} writes={writes:?}", fixtures::SEED);
        assert!(decoded > 31 * 1024 * 1024 && serialized > 31 * 1024 * 1024);
    }
}

#[test]
fn gate_request_failure_rolls_back_all_tasks_gate_request_and_inbox() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "gate".into(),
        root.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.tasks = vec![
        load_audit::task("one", TaskStatus::Pending, None),
        load_audit::task("two", TaskStatus::Pending, None),
    ];
    drop(fixtures::database(root.path(), &plan));
    let store = PlanStore::open(root.path().join("plans.db")).unwrap();
    store.database.pending_changes.lock().take(&plan.id);
    store.database.connection.lock().execute_batch("CREATE TRIGGER reject_gate_revision BEFORE UPDATE ON plans WHEN json_extract(NEW.header_json,'$._revision') != coalesce(json_extract(OLD.header_json,'$._revision'),0) BEGIN SELECT RAISE(ABORT,'injected gate commit failure'); END;").unwrap();
    let error = store
        .ask_gate_with_planner_request(
            &plan.id,
            "question",
            "context",
            &["one".into(), "two".into()],
            None,
            None,
            None,
            20,
        )
        .unwrap_err();
    assert!(error.to_string().contains("injected gate commit failure"));
    assert!(store.take_committed_change(&plan.id).is_none());
    crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&store.database);
    drop(store);
    let reopened = PlanDatabase::open(root.path().join("plans.db")).unwrap();
    assert_eq!(reopened.load_plan(&plan.id).unwrap().unwrap(), plan);
}

#[test]
fn concurrent_gate_answers_commit_one_answer_and_one_resume_milestone() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "gate".into(),
        root.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.tasks
        .push(load_audit::task("task", TaskStatus::Pending, None));
    drop(fixtures::database(root.path(), &plan));
    let store = Arc::new(PlanStore::open(root.path().join("plans.db")).unwrap());
    let gate = store
        .ask_gate(&plan.id, "question", "", &["task".into()], None, None, 20)
        .unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let handles: Vec<_> = ["one", "two"]
        .into_iter()
        .map(|answer| {
            let store = store.clone();
            let barrier = barrier.clone();
            let plan_id = plan.id.clone();
            let gate_id = gate.id.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.answer_gate(&plan_id, &gate_id, answer, None, 21)
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
            .filter(|result| matches!(result, Err(PlanError::GateAlreadyAnswered)))
            .count(),
        1
    );
    let stored = store.database.load_plan(&plan.id).unwrap().unwrap();
    let accepted = results
        .iter()
        .find_map(|result| result.as_ref().ok())
        .unwrap();
    assert_eq!(stored.gates, vec![accepted.clone()]);
    assert_eq!(stored.tasks[0].milestones.len(), 2);
    assert_eq!(stored.tasks[0].status, TaskStatus::Pending);
}
