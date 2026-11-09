use super::*;
use crate::service_plan::store_db::hot_write::tests::fixtures;
use crate::service_plan::{load_audit, PlanStore};
use std::sync::{Arc, Barrier};

fn request(ops: Vec<BatchOp>) -> BatchRequest {
    BatchRequest { key: None, ops }
}
fn goal(vector: &str) -> BatchOp {
    BatchOp::SetGoal {
        client_id: None,
        vector: vector.into(),
        vision: "vision".into(),
        constraints: vec![],
        postulates: vec![],
    }
}
fn handoff() -> BatchOp {
    BatchOp::AddHandoff {
        client_id: Some("handoff".into()),
        mode: None,
        content: serde_json::json!({}),
        body: "durable body".into(),
        task: None,
        from: None,
        to: None,
    }
}

#[test]
fn mixed_batch_rows_and_bytes_ignore_unrelated_history_and_bodies() {
    let _reminder_clock = crate::service_plan::hints::fixed_reminder_count(1);
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
        let result = store
            .apply_batch(
                &plan.id,
                request(vec![
                    goal("batch"),
                    BatchOp::IngestSections {
                        body: "# Incoming\n\nSelected body".into(),
                    },
                    BatchOp::UpsertSpec {
                        client_id: Some("spec".into()),
                        spec_id: None,
                        title: "spec".into(),
                    },
                    BatchOp::UpsertSpecSection {
                        client_id: None,
                        spec: "$spec".into(),
                        section_id: None,
                        title: "part".into(),
                        body: Some("spec body".into()),
                    },
                    handoff(),
                    BatchOp::CreatePlanFromSpec {
                        client_id: Some("derived".into()),
                        spec: "$spec".into(),
                        title: "derived".into(),
                    },
                ]),
                20,
            )
            .unwrap();
        let work = super::super::take_work();
        let writes = fixtures::audit_counts(&store.database);
        let response_bytes = serde_json::to_vec(&result).unwrap().len();
        println!("fixture={} tasks={} base_content_bytes={} operation=mixed_batch strides={strides} unrelated_attempts={attempts} unrelated_body_bytes={body_bytes} writes={writes:?} task_header_attempt_milestone_stride_decoded_serialized_projected_hashed_section_element_heartbeat_spec_file={work:?} response_bytes={response_bytes}",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES);
        assert!(store.plan_loads_for_test().is_empty());
        assert_eq!(
            (work[0], work[2], work[3], work[4], work[9], work[10], work[12], work[13]),
            (0, 0, 0, 0, 1, 1, 2, 12)
        );
        assert!(work[5] < 2048 && work[6] < 8192 && response_bytes < 2048);
        let measured = (work, writes, response_bytes);
        if let Some(prior) = prior {
            assert_eq!(prior, measured);
        }
        prior = Some(measured);
        let derived = result.ids["derived"].clone();
        crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(
            &store.database,
        );
        drop(store);
        let reopened = PlanDatabase::open(root.path().join("plans.db")).unwrap();
        let stored = reopened.load_plan(&plan.id).unwrap().unwrap();
        assert_eq!(stored.tasks, plan.tasks);
        assert_eq!(stored.strides, plan.strides);
        assert_eq!(stored.elements[0].content["body"], "durable body");
        assert_eq!(stored.goal.unwrap().vector, "batch");
        let derived = reopened.load_plan(&derived).unwrap().unwrap();
        assert_eq!(derived.status, crate::plan::PlanStatus::Archived);
        assert!(derived.team_id.is_none());
        let baseline_root = tempfile::tempdir().unwrap();
        let baseline = fixtures::database(baseline_root.path(), &plan);
        fixtures::baseline_operation(&baseline, &plan.id, "batch_goal", |plan| {
            plan.goal = Some(crate::plan::Goal {
                vector: "batch".into(),
                vision: "vision".into(),
                constraints: vec![],
                postulates: vec![],
            })
        });
    }
}

#[test]
fn later_batch_failure_rolls_back_specs_derived_plan_and_handoff_file() {
    let root = tempfile::tempdir().unwrap();
    let plan = crate::plan::Plan::new(
        "rollback".into(),
        root.path().display().to_string(),
        AgentRef::from_tab("owner"),
    );
    let database = fixtures::database(root.path(), &plan);
    fixtures::audit(&database);
    let before: usize = database
        .connection
        .lock()
        .query_row("SELECT count(*) FROM plans", [], |row| row.get(0))
        .unwrap();
    let result = database.commit_hot_batch(
        &plan.id,
        request(vec![
            goal("rolled back"),
            handoff(),
            BatchOp::UpsertSpec {
                client_id: Some("new".into()),
                spec_id: None,
                title: "rollback spec".into(),
            },
            BatchOp::CreatePlanFromSpec {
                client_id: None,
                spec: "$new".into(),
                title: "rollback child".into(),
            },
            BatchOp::SetTaskBody {
                task: "missing".into(),
                body: "fail".into(),
            },
        ]),
        vec![],
        20,
    );
    assert!(matches!(result, Err(PlanError::TaskNotFound)));
    assert!(fixtures::audit_counts(&database).is_empty());
    assert_eq!(
        database
            .connection
            .lock()
            .query_row("SELECT count(*) FROM plans", [], |row| row
                .get::<_, usize>(0))
            .unwrap(),
        before
    );
    let stored = database.load_plan(&plan.id).unwrap().unwrap();
    assert!(stored.goal.is_none() && stored.elements.is_empty() && stored.owner_history.is_empty());
    assert_eq!(stored.updated_at, plan.updated_at);
    let directory = crate::service_plan::body_files::handoff_path(&plan, "unused")
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    assert!(directory
        .read_dir()
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(true));
}

#[test]
fn competing_handoff_batches_allocate_distinct_sequences_and_reopen() {
    let root = tempfile::tempdir().unwrap();
    let plan = crate::plan::Plan::new(
        "concurrent".into(),
        root.path().display().to_string(),
        AgentRef::from_tab("owner"),
    );
    let database = Arc::new(fixtures::database(root.path(), &plan));
    let barrier = Arc::new(Barrier::new(2));
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let database = database.clone();
            let barrier = barrier.clone();
            let id = plan.id.clone();
            std::thread::spawn(move || {
                barrier.wait();
                database
                    .commit_hot_batch(&id, request(vec![handoff()]), vec![], 20)
                    .unwrap();
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&database);
    drop(database);
    let reopened = PlanDatabase::open(root.path().join("plans.db")).unwrap();
    let stored = reopened.load_plan(&plan.id).unwrap().unwrap();
    let mut sequences: Vec<_> = stored
        .elements
        .iter()
        .map(|element| element.content["seq"].as_u64().unwrap())
        .collect();
    sequences.sort();
    assert_eq!(sequences, vec![1, 2]);
    assert_eq!(stored.owner_history.len(), 2);
}
