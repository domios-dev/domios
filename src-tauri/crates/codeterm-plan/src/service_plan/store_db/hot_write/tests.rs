use super::*;
use crate::plan::{AgentRef, OrchestratorRef, Plan, TaskStatus};
use crate::service_plan::{load_audit, PlanStore};
use std::sync::{Arc, Barrier};
use std::time::Instant;

const SEED: &str = "sql-hot-1162-v1";
const TASKS: usize = 1162;
const CONTENT_BYTES: usize = 33_344_717;

fn milestone(id: &str) -> Milestone {
    Milestone {
        id: id.into(),
        from: AgentRef::from_tab("worker"),
        kind: "decision".into(),
        text: "bounded append".into(),
        ts: 20,
        links: vec![],
        loop_id: None,
        iteration: None,
        canvas: Default::default(),
        attachments: vec![],
    }
}

fn fixture(root: &std::path::Path, strides: usize, attempts: usize) -> Plan {
    let mut plan = Plan::new(
        SEED.into(),
        root.display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.id = SEED.into();
    plan.created_at = 1;
    plan.updated_at = 1;
    plan.tasks = (0..TASKS)
        .map(|i| {
            let mut task = load_audit::task(&format!("task_{i:04}"), TaskStatus::Pending, None);
            task.handle = Some(format!("fixture-{i:04}"));
            task.updated_at = 1;
            task
        })
        .collect();
    let mut current = load_audit::attempt("current", "worker");
    current.session_id = Some("session".into());
    current.session_binding_nonce = Some("nonce".into());
    plan.tasks[0].execution_attempts.push(current);
    let empty_bytes = serde_json::to_vec(&plan).unwrap().len();
    let fill = CONTENT_BYTES - empty_bytes;
    for (i, task) in plan.tasks.iter_mut().enumerate() {
        task.body = "x".repeat(fill / TASKS + usize::from(i < fill % TASKS));
    }
    assert_eq!(serde_json::to_vec(&plan).unwrap().len(), CONTENT_BYTES);
    plan.strides = (0..strides)
        .map(|i| milestone(&format!("history-{i}")))
        .collect();
    plan.tasks[1].execution_attempts = (0..attempts)
        .map(|i| {
            let mut attempt = load_audit::attempt(&format!("old-{i}"), "unrelated");
            attempt.closed_at = Some(2);
            attempt
        })
        .collect();
    plan
}

fn database(root: &std::path::Path, plan: &Plan) -> PlanDatabase {
    let database = PlanDatabase::open(root.join("plans.db")).unwrap();
    database.persist_plan_rows(plan, None).unwrap();
    let mut spec = crate::plan::Spec::new("owned spec".into(), plan.project_path.clone());
    spec.plan_id = Some(plan.id.clone());
    database.persist_spec(&spec).unwrap();
    database
}

fn role(value: &str) -> HotChange {
    HotChange::Task {
        selector: "task_0000".into(),
        change: TaskMetadata::RoleGuidance(Some(Some(value.into()))),
    }
}

fn audit(database: &PlanDatabase) {
    let connection = database.connection.lock();
    connection
        .execute_batch("CREATE TABLE write_audit(table_name TEXT);")
        .unwrap();
    for table in [
        "plans",
        "plan_tasks",
        "plan_task_milestones",
        "plan_task_attempts",
        "plan_strides",
        "specs",
    ] {
        for action in ["INSERT", "UPDATE", "DELETE"] {
            connection.execute_batch(&format!("CREATE TRIGGER audit_{table}_{action} AFTER {action} ON {table} BEGIN INSERT INTO write_audit VALUES ('{table}'); END;")).unwrap();
        }
    }
}

fn audit_counts(database: &PlanDatabase) -> Vec<(String, usize)> {
    let connection = database.connection.lock();
    let mut statement = connection
        .prepare(
            "SELECT table_name, count(*) FROM write_audit GROUP BY table_name ORDER BY table_name",
        )
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn hot_work_stays_bounded_with_independently_growing_history() {
    let mut prior = None;
    for (strides, attempts) in [(0, 0), (2048, 0), (0, 2048)] {
        let directory = tempfile::tempdir().unwrap();
        let plan = fixture(directory.path(), strides, attempts);
        let actual_bytes = serde_json::to_vec(&plan).unwrap().len();
        let database = database(directory.path(), &plan);
        audit(&database);
        database
            .connection
            .lock()
            .execute(
                "UPDATE plan_fts SET body='untouched FTS sentinel' WHERE plan_id=?1",
                [&plan.id],
            )
            .unwrap();
        let fts_before: (i64, String) = database
            .connection
            .lock()
            .query_row(
                "SELECT rowid, body FROM plan_fts WHERE plan_id=?1",
                [&plan.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let started = Instant::now();
        let metadata = database
            .commit_hot_changes(&plan.id, vec![role("metadata")], 20)
            .unwrap();
        println!("fixture={SEED} tasks={TASKS} base_content_bytes={CONTENT_BYTES} actual_bytes={actual_bytes} strides={strides} unrelated_attempts={attempts} metadata_us={} receipt={metadata:?}", started.elapsed().as_micros());
        assert_eq!(
            audit_counts(&database),
            vec![("plan_tasks".into(), 1), ("plans".into(), 1)]
        );
        assert!(metadata.decoded_bytes < 32 * 1024 && metadata.serialized_bytes < 32 * 1024);
        let started = Instant::now();
        let text = database
            .commit_hot_changes(&plan.id, vec![role("changed guidance text")], 21)
            .unwrap();
        println!("text_us={} receipt={text:?}", started.elapsed().as_micros());
        let started = Instant::now();
        let stride = database
            .commit_hot_changes(
                &plan.id,
                vec![HotChange::Stride(milestone("new-stride"))],
                22,
            )
            .unwrap();
        println!(
            "stride_us={} receipt={stride:?}",
            started.elapsed().as_micros()
        );
        assert_eq!((stride.strides, stride.decoded_bytes), (1, 0));
        let started = Instant::now();
        let (_, attempt) = database
            .change_attempt(
                &plan.id,
                "task_0000",
                "worker",
                Some("current"),
                23,
                |attempt| {
                    attempt.unwrap().session_model = Some("observed".into());
                    Ok(Some(()))
                },
            )
            .unwrap();
        println!(
            "attempt_us={} receipt={attempt:?}",
            started.elapsed().as_micros()
        );
        assert_eq!((attempt.task_rows, attempt.attempts), (1, 1));
        let started = Instant::now();
        let review = database
            .commit_hot_changes(
                &plan.id,
                vec![HotChange::Task {
                    selector: "task_0000".into(),
                    change: TaskMetadata::ReviewOverride(ReviewOverride {
                        owner: "owner".into(),
                        reason: "reason".into(),
                        reproduction: "reproduction".into(),
                        at: 24,
                    }),
                }],
                24,
            )
            .unwrap();
        println!(
            "review_override_us={} receipt={review:?}",
            started.elapsed().as_micros()
        );
        let started = Instant::now();
        let record = database
            .commit_hot_changes(
                &plan.id,
                vec![HotChange::Milestone {
                    task_id: "task_0000".into(),
                    milestone: milestone("new-record"),
                    add_kind: true,
                }],
                25,
            )
            .unwrap();
        println!(
            "milestone_us={} receipt={record:?}",
            started.elapsed().as_micros()
        );
        assert_eq!(
            (record.task_rows, record.milestones, record.search.len()),
            (1, 1, 1)
        );
        let fts_after: (i64, String) = database
            .connection
            .lock()
            .query_row(
                "SELECT rowid, body FROM plan_fts WHERE plan_id=?1",
                [&plan.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(fts_before, fts_after);
        assert!(!audit_counts(&database)
            .iter()
            .any(|(table, _)| table == "specs"));
        let measured = [metadata, text, stride, attempt, review, record].map(|r| {
            (
                r.task_rows,
                r.header_rows,
                r.attempts,
                r.milestones,
                r.strides,
                r.decoded_bytes,
                r.serialized_bytes,
            )
        });
        if let Some(prior) = prior {
            assert_eq!(prior, measured);
        }
        prior = Some(measured);
        let loaded = database.load_plan(&plan.id).unwrap().unwrap();
        assert_eq!(loaded.strides.len(), strides + 1);
        assert_eq!(loaded.tasks[1].execution_attempts.len(), attempts);
        assert_eq!(loaded.tasks[0].milestones.len(), 1);
        let baseline_root = tempfile::tempdir().unwrap();
        let baseline = database_for_baseline(baseline_root.path(), &plan);
        baseline_operation(&baseline, &plan.id, "metadata", |plan| {
            plan.tasks[0].role_guidance = Some("metadata".into());
            plan.tasks[0].updated_at = 20;
        });
        baseline_operation(&baseline, &plan.id, "text", |plan| {
            plan.tasks[0].role_guidance = Some("changed guidance text".into());
            plan.tasks[0].updated_at = 21;
        });
        baseline_operation(&baseline, &plan.id, "stride", |plan| {
            plan.strides.push(milestone("new-stride"));
        });
        baseline_operation(&baseline, &plan.id, "attempt", |plan| {
            plan.tasks[0].execution_attempts[0].session_model = Some("observed".into());
            plan.tasks[0].updated_at = 23;
        });
        baseline_operation(&baseline, &plan.id, "review_override", |plan| {
            plan.tasks[0].review_override = Some(ReviewOverride {
                owner: "owner".into(),
                reason: "reason".into(),
                reproduction: "reproduction".into(),
                at: 24,
            });
            plan.tasks[0].updated_at = 24;
        });
        baseline_operation(&baseline, &plan.id, "milestone", |plan| {
            plan.tasks[0].milestones.push(milestone("new-record"));
        });
    }
}

fn database_for_baseline(root: &std::path::Path, plan: &Plan) -> PlanDatabase {
    database(root, plan)
}

fn baseline_operation(
    database: &PlanDatabase,
    id: &str,
    kind: &str,
    mutate: impl FnOnce(&mut Plan),
) {
    let started = Instant::now();
    let mut plan = database.load_plan(id).unwrap().unwrap();
    let decoded = serde_json::to_vec(&plan).unwrap().len();
    mutate(&mut plan);
    plan.updated_at += 1;
    let rows = store_rows::split_plan(serde_json::to_value(&plan).unwrap()).unwrap();
    let serialized = rows.header.len()
        + rows
            .tasks
            .iter()
            .map(|task| {
                task.document.len()
                    + task.milestones.iter().map(String::len).sum::<usize>()
                    + task
                        .attempts
                        .iter()
                        .map(|attempt| attempt.document.len())
                        .sum::<usize>()
            })
            .sum::<usize>()
        + rows.strides.iter().map(String::len).sum::<usize>();
    let writes = database.persist_plan_rows(&plan, None).unwrap();
    println!("baseline=load_plan+persist_plan_rows operation={kind} strides={} unrelated_attempts={} decoded_reconstructed_bytes={decoded} serialized_row_bytes={serialized} elapsed_us={} writes={writes:?}", plan.strides.len(), plan.tasks[1].execution_attempts.len(), started.elapsed().as_micros());
    assert!(serialized > 31 * 1024 * 1024);
}

#[test]
fn mixed_partial_sql_failure_rolls_back_rows_and_discards_receipt() {
    let directory = tempfile::tempdir().unwrap();
    let plan = fixture(directory.path(), 0, 0);
    let database = database(directory.path(), &plan);
    audit(&database);
    database.connection.lock().execute_batch("CREATE TRIGGER fail_stride BEFORE INSERT ON plan_strides BEGIN SELECT RAISE(ABORT, 'injected after milestone write'); END;").unwrap();
    let result = database.commit_hot_changes(
        &plan.id,
        vec![
            role("rolled back"),
            HotChange::Milestone {
                task_id: "task_0000".into(),
                milestone: milestone("rolled-back-record"),
                add_kind: true,
            },
            HotChange::Stride(milestone("failure")),
        ],
        20,
    );
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("injected after milestone write"));
    assert!(audit_counts(&database).is_empty());
    drop(database);
    let reopened = PlanDatabase::open(directory.path().join("plans.db")).unwrap();
    assert_eq!(reopened.load_plan(&plan.id).unwrap().unwrap(), plan);
    let failed: usize = reopened
        .connection
        .lock()
        .query_row(
            "SELECT count(*) FROM plan_operations WHERE state='failed'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(failed, 1);
}

#[test]
fn public_attempt_stale_binding_is_noop_and_committed_cache_reloads() {
    let directory = tempfile::tempdir().unwrap();
    let plan = fixture(directory.path(), 0, 0);
    drop(database(directory.path(), &plan));
    let store = PlanStore::open(directory.path().join("plans.db")).unwrap();
    assert!(store.get(&plan.id).is_some());
    load_audit::start(&store);
    assert!(!store
        .update_attempt_session_model_if_unknown(
            &plan.id,
            "task_0000",
            "worker",
            "current",
            "session",
            "stale",
            "model",
            20
        )
        .unwrap());
    assert!(store
        .update_attempt_session_model_if_unknown(
            &plan.id,
            "task_0000",
            "worker",
            "current",
            "session",
            "nonce",
            "model",
            21
        )
        .unwrap());
    assert!(store.plan_loads_for_test().is_empty());
    assert_eq!(
        store.get(&plan.id).unwrap().tasks[0].execution_attempts[0]
            .session_model
            .as_deref(),
        Some("model")
    );
    drop(store);
    let reopened = PlanStore::open(directory.path().join("plans.db")).unwrap();
    assert_eq!(
        reopened.get(&plan.id).unwrap().tasks[0].execution_attempts[0]
            .session_model
            .as_deref(),
        Some("model")
    );
}

#[test]
fn concurrent_transactions_preserve_both_accepted_changes() {
    let directory = tempfile::tempdir().unwrap();
    let plan = fixture(directory.path(), 0, 0);
    drop(database(directory.path(), &plan));
    let barrier = Arc::new(Barrier::new(2));
    let writers: Vec<_> = (0..2)
        .map(|i| {
            let db = PlanDatabase::open(directory.path().join("plans.db")).unwrap();
            let barrier = barrier.clone();
            let id = plan.id.clone();
            std::thread::spawn(move || {
                barrier.wait();
                db.commit_hot_changes(
                    &id,
                    vec![HotChange::Task {
                        selector: format!("task_{i:04}"),
                        change: TaskMetadata::RoleGuidance(Some(Some(format!("writer-{i}")))),
                    }],
                    20 + i,
                )
                .unwrap();
            })
        })
        .collect();
    for writer in writers {
        writer.join().unwrap();
    }
    let reopened = PlanDatabase::open(directory.path().join("plans.db")).unwrap();
    let loaded = reopened.load_plan(&plan.id).unwrap().unwrap();
    assert_eq!(loaded.tasks[0].role_guidance.as_deref(), Some("writer-0"));
    assert_eq!(loaded.tasks[1].role_guidance.as_deref(), Some("writer-1"));
}

#[test]
fn committed_milestone_indexes_only_its_unit_and_failure_publishes_nothing() {
    use crate::service_plan::search_index;
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixture(directory.path(), 0, 0);
    plan.tasks.truncate(2);
    for task in &mut plan.tasks {
        task.body.clear();
    }
    drop(database(directory.path(), &plan));
    search_index::init_search_port(Arc::new(crate::service_plan::test_support::TestSearchPort));
    let store = PlanStore::open(directory.path().join("plans.db")).unwrap();
    store.mirror_plan_index(&plan).unwrap();
    search_index::flush_index_queue_for_test(directory.path());
    let index =
        rusqlite::Connection::open(search_index::project_index_path(directory.path())).unwrap();
    index.execute_batch("CREATE TABLE index_audit(id TEXT); CREATE TRIGGER unit_insert AFTER INSERT ON mem_items BEGIN INSERT INTO index_audit VALUES (NEW.id); END; CREATE TRIGGER unit_delete AFTER DELETE ON mem_items BEGIN INSERT INTO index_audit VALUES (OLD.id); END;").unwrap();
    load_audit::start(&store);
    let id = store
        .add_milestone(
            &plan.id,
            "task_0000",
            "decision",
            "indexed once",
            AgentRef::from_tab("worker"),
            true,
            20,
        )
        .unwrap();
    assert!(store.plan_loads_for_test().is_empty());
    search_index::flush_index_queue_for_test(directory.path());
    let indexed: Vec<String> = index
        .prepare("SELECT id FROM index_audit")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(indexed, vec![format!("planunit_{}_stride_{id}", plan.id)]);
    store.database.connection.lock().execute_batch("CREATE TRIGGER fail_header BEFORE UPDATE OF updated_at ON plans BEGIN SELECT RAISE(ABORT, 'no commit'); END;").unwrap();
    assert!(store
        .add_milestone(
            &plan.id,
            "task_0000",
            "decision",
            "must not be indexed",
            AgentRef::from_tab("worker"),
            false,
            21
        )
        .is_err());
    search_index::flush_index_queue_for_test(directory.path());
    let count: usize = index
        .query_row("SELECT count(*) FROM index_audit", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(store.get(&plan.id).unwrap().tasks[0].milestones.len(), 1);
}
