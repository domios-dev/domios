use super::*;
use crate::plan::{AgentRef, OrchestratorRef, Plan, TaskStatus};
use crate::service_plan::{load_audit, PlanStore};
use std::sync::{Arc, Barrier};
use std::time::Instant;

pub(in crate::service_plan::store_db) mod fixtures;
use fixtures::*;

#[test]
fn hot_work_stays_bounded_with_independently_growing_history() {
    let mut prior = None;
    for (strides, attempts) in [(0, 0), (2048, 0), (0, 2048)] {
        let directory = tempfile::tempdir().unwrap();
        let plan = fixture(directory.path(), strides, attempts);
        let actual_bytes = serde_json::to_vec(&plan).unwrap().len();
        let database = database(directory.path(), &plan);
        audit(&database);
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
        assert_and_clear_writes(&database, &metadata);
        let started = Instant::now();
        let text = database
            .commit_hot_changes(
                &plan.id,
                vec![HotChange::Fields {
                    selector: "task_0000".into(),
                    fields: TaskFields {
                        body: Some("changed searchable task text".into()),
                        ..Default::default()
                    },
                }],
                21,
            )
            .unwrap();
        println!("text_us={} receipt={text:?}", started.elapsed().as_micros());
        assert_and_clear_writes(&database, &text);
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
        assert_and_clear_writes(&database, &stride);
        let started = Instant::now();
        let (_, attempt) = database
            .change_attempt(
                &plan.id,
                "task_0000",
                AttemptTarget {
                    tab: "worker",
                    id: Some("current"),
                    ..Default::default()
                },
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
        assert_and_clear_writes(&database, &attempt);
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
        assert_and_clear_writes(&database, &review);
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
        assert_and_clear_writes(&database, &record);
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
            plan.tasks[0].body = "changed searchable task text".into();
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
    {
        let connection = database.connection.lock();
        for table in [
            "plans",
            "plan_tasks",
            "plan_task_milestones",
            "plan_task_attempts",
            "plan_strides",
            "specs",
        ] {
            for action in ["INSERT", "UPDATE", "DELETE"] {
                connection
                    .execute_batch(&format!("DROP TRIGGER audit_{table}_{action};"))
                    .unwrap();
            }
        }
        connection
            .execute_batch("DROP TRIGGER fail_stride; DROP TABLE write_audit;")
            .unwrap();
    }
    crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&database);
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
    crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&store.database);
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

#[test]
fn real_review_hold_has_bounded_work_as_unrelated_history_grows() {
    let mut prior = None;
    for (strides, attempts) in [(0, 0), (2048, 0), (0, 2048)] {
        let root = tempfile::tempdir().unwrap();
        let mut plan = fixture(root.path(), strides, attempts);
        plan.tasks[0].status = TaskStatus::InReview;
        let actual_bytes = serde_json::to_vec(&plan).unwrap().len();
        drop(database(root.path(), &plan));
        let store = PlanStore::open(root.path().join("plans.db")).unwrap();
        load_audit::start(&store);
        let started = Instant::now();
        let outcome = store
            .hold_review(
                &plan.id,
                "task_0000",
                "review hold note",
                AgentRef::from_tab("owner"),
                30,
            )
            .unwrap();
        let work = take_work();
        println!("fixture={SEED} operation=real_review_hold actual_bytes={actual_bytes} strides={strides} unrelated_attempts={attempts} elapsed_us={} task_rows={} header_rows={} attempt_rows={} milestone_rows={} stride_rows={} decoded_bytes={} serialized_bytes={} projected_text_bytes={} hashed_bytes={}", started.elapsed().as_micros(), work[0],work[1],work[2],work[3],work[4],work[5],work[6],work[7],work[8]);
        assert_eq!(&work[..5], &[2, 1, 0, 1, 0]);
        assert!(work[5] < 32 * 1024 && work[6] < 32 * 1024);
        assert!(store.plan_loads_for_test().is_empty());
        if let Some(prior) = prior {
            assert_eq!(prior, work);
        }
        prior = Some(work);
        let stored = store.get(&plan.id).unwrap();
        assert_eq!(stored.tasks[0].status, TaskStatus::InReview);
        assert_eq!(
            stored.tasks[0].review_evidence.last().unwrap().note,
            "review hold note"
        );
        assert_eq!(
            stored.tasks[0].milestones.last().unwrap().id,
            outcome.milestone_id
        );
    }
}

#[test]
fn stale_owner_and_denied_review_status_leave_every_row_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = fixture(root.path(), 0, 0);
    plan.tasks[0].status = TaskStatus::InReview;
    plan.tasks[0].interruption.owner_tab_id = Some("worker".into());
    plan.tasks[0].interruption.owner_session_id = Some("session".into());
    plan.tasks[0].interruption.owner_epoch = 2;
    drop(database(root.path(), &plan));
    let store = PlanStore::open(root.path().join("plans.db")).unwrap();
    let stale = crate::service_plan::interruption::TaskMutationAuthority {
        tab_id: "worker".into(),
        session_id: "session".into(),
        session_binding_nonce: "nonce".into(),
        owner_epoch: 1,
    };
    let error = store
        .close_task_authorized(
            &plan.id,
            "task_0000",
            Some("must roll back"),
            AgentRef::from_tab("worker"),
            &stale,
            20,
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("stale task mutation owner epoch"));
    let error = store
        .commit_hot_changes(
            &plan.id,
            vec![
                role("must roll back"),
                HotChange::Fields {
                    selector: "task_0000".into(),
                    fields: TaskFields {
                        status: Some("done".into()),
                        ..Default::default()
                    },
                },
            ],
            21,
        )
        .unwrap_err();
    assert!(matches!(
        error,
        PlanError::TransitionDenied {
            from: TaskStatus::InReview,
            to: TaskStatus::Done
        }
    ));
    crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&store.database);
    drop(store);
    let reopened = PlanStore::open(root.path().join("plans.db")).unwrap();
    assert_eq!(reopened.get(&plan.id).unwrap(), plan);
}

#[test]
fn task_metadata_and_searchable_text_update_only_the_required_fts_record() {
    use crate::service_plan::search_index;
    let root = tempfile::tempdir().unwrap();
    let mut plan = fixture(root.path(), 0, 0);
    plan.tasks.truncate(2);
    for task in &mut plan.tasks {
        task.body.clear();
    }
    drop(database(root.path(), &plan));
    search_index::init_search_port(Arc::new(crate::service_plan::test_support::TestSearchPort));
    let store = PlanStore::open(root.path().join("plans.db")).unwrap();
    store.mirror_plan_index(&plan).unwrap();
    search_index::flush_index_queue_for_test(root.path());
    let index = rusqlite::Connection::open(search_index::project_index_path(root.path())).unwrap();
    let target = format!("planunit_{}_task_task_0000", plan.id);
    let unrelated = format!("planunit_{}_task_task_0001", plan.id);
    index
        .execute(
            "UPDATE mem_fts SET body='FTS sentinel' WHERE mem_id IN (?1,?2)",
            params![target, unrelated],
        )
        .unwrap();
    store
        .commit_hot_changes(
            &plan.id,
            vec![HotChange::Fields {
                selector: "task_0000".into(),
                fields: TaskFields {
                    priority: Some("p1".into()),
                    ..Default::default()
                },
            }],
            20,
        )
        .unwrap();
    search_index::flush_index_queue_for_test(root.path());
    let plan_unit = format!("planunit_{}_plan_{}", plan.id, plan.id);
    assert_eq!(
        index
            .query_row(
                "SELECT json_extract(source,'$.updated_at') FROM mem_items WHERE id=?1",
                [&plan_unit],
                |row| row.get::<_, u64>(0)
            )
            .unwrap(),
        20
    );
    assert_eq!(
        index
            .query_row(
                "SELECT body FROM mem_fts WHERE mem_id=?1",
                [&target],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
        "FTS sentinel"
    );
    store
        .commit_hot_changes(
            &plan.id,
            vec![HotChange::Fields {
                selector: "task_0000".into(),
                fields: TaskFields {
                    body: Some("new searchable content".into()),
                    ..Default::default()
                },
            }],
            21,
        )
        .unwrap();
    search_index::flush_index_queue_for_test(root.path());
    assert!(index
        .query_row(
            "SELECT body FROM mem_fts WHERE mem_id=?1",
            [&target],
            |row| row.get::<_, String>(0)
        )
        .unwrap()
        .contains("new searchable content"));
    assert_eq!(
        index
            .query_row(
                "SELECT body FROM mem_fts WHERE mem_id=?1",
                [&unrelated],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
        "FTS sentinel"
    );
}

#[test]
fn public_hot_batch_is_atomic_and_resolves_clients_without_loading_plan() {
    use crate::service_plan::batch::{BatchOp, BatchRequest};
    let root = tempfile::tempdir().unwrap();
    let mut plan = fixture(root.path(), 0, 0);
    plan.tasks.truncate(2);
    drop(database(root.path(), &plan));
    let store = PlanStore::open(root.path().join("plans.db")).unwrap();
    load_audit::start(&store);
    let bad = BatchRequest {
        key: None,
        ops: vec![
            BatchOp::SetTaskBody {
                task: "task_0000".into(),
                body: "must roll back".into(),
            },
            BatchOp::AddMilestone {
                client_id: None,
                task: "missing".into(),
                kind: "decision".into(),
                text: "failure".into(),
                from: None,
                key: None,
                links: vec![],
            },
        ],
    };
    assert!(matches!(
        store.apply_batch(&plan.id, bad, 20),
        Err(PlanError::TaskNotFound)
    ));
    assert!(store.plan_loads_for_test().is_empty());
    assert_eq!(store.database.load_plan(&plan.id).unwrap().unwrap(), plan);
    let request = BatchRequest {
        key: Some("batch-key".into()),
        ops: vec![
            BatchOp::SetTaskSections {
                client_id: Some("target".into()),
                task: "task_0000".into(),
                sections: vec![],
            },
            BatchOp::SetTaskBody {
                task: "$target".into(),
                body: "committed batch body".into(),
            },
            BatchOp::AddMilestone {
                client_id: Some("record".into()),
                task: "$target".into(),
                kind: "decision".into(),
                text: "once".into(),
                from: None,
                key: None,
                links: vec![],
            },
        ],
    };
    let result = store.apply_batch(&plan.id, request.clone(), 21).unwrap();
    let replay = store.apply_batch(&plan.id, request, 22).unwrap();
    assert_eq!(result.milestone_ids, replay.milestone_ids);
    assert_eq!(result.task_ids, vec!["task_0000"]);
    assert!(store.plan_loads_for_test().is_empty());
    let stored = store.database.load_plan(&plan.id).unwrap().unwrap();
    assert_eq!(stored.tasks[0].body, "committed batch body");
    assert_eq!(stored.tasks[0].milestones.len(), 1);
}

#[test]
fn milestone_edit_and_remove_preserve_surviving_rows_and_append_order() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = fixture(root.path(), 0, 0);
    plan.tasks.truncate(2);
    drop(database(root.path(), &plan));
    let store = PlanStore::open(root.path().join("plans.db")).unwrap();
    let first = store
        .add_milestone(
            &plan.id,
            "task_0000",
            "decision",
            "first",
            AgentRef::from_tab("worker"),
            true,
            20,
        )
        .unwrap();
    let second = store
        .add_milestone(
            &plan.id,
            "task_0000",
            "decision",
            "second",
            AgentRef::from_tab("worker"),
            false,
            21,
        )
        .unwrap();
    store
        .update_milestone(&plan.id, "task_0000", &second, None, Some("edited"), 22)
        .unwrap();
    store
        .rm_milestone(&plan.id, "task_0000", &first, 23)
        .unwrap();
    let third = store
        .add_milestone(
            &plan.id,
            "task_0000",
            "decision",
            "third",
            AgentRef::from_tab("worker"),
            false,
            24,
        )
        .unwrap();
    let mut loaded = store.database.load_plan(&plan.id).unwrap().unwrap();
    assert_eq!(
        loaded.tasks[0]
            .milestones
            .iter()
            .map(|m| m.id.as_str())
            .collect::<Vec<_>>(),
        vec![second.as_str(), third.as_str()]
    );
    assert_eq!(loaded.tasks[0].milestones[0].text, "edited");
    loaded.tasks[0].milestones.push(milestone("legacy-append"));
    store.database.persist_plan_rows(&loaded, None).unwrap();
    let reopened = store.database.load_plan(&plan.id).unwrap().unwrap();
    assert_eq!(reopened, loaded);
}

#[test]
fn retired_plan_fts_is_dropped_only_after_exact_schema_admission() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = fixture(root.path(), 0, 0);
    plan.tasks.truncate(2);
    let mut connection = rusqlite::Connection::open(root.path().join("plans.db")).unwrap();
    connection
        .execute_batch(include_str!("../migrations/schemas/001.sql"))
        .unwrap();
    connection.execute_batch("PRAGMA user_version=1;").unwrap();
    let rows = store_rows::split_plan(serde_json::to_value(&plan).unwrap()).unwrap();
    let tx = connection.transaction().unwrap();
    super::super::upsert_plan(&tx, &plan, &rows, None).unwrap();
    tx.commit().unwrap();
    drop(connection);
    let reopened = PlanDatabase::open(root.path().join("plans.db")).unwrap();
    assert_eq!(reopened.load_plan(&plan.id).unwrap().unwrap(), plan);
    let count: usize = reopened
        .connection
        .lock()
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name GLOB 'plan_fts*'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn successful_hot_commit_preserves_canonical_path_normalization() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = fixture(root.path(), 0, 0);
    plan.tasks.truncate(2);
    plan.canonical_project_path.clear();
    let database = database(root.path(), &plan);
    database
        .commit_hot_changes(&plan.id, vec![role("normalized")], 20)
        .unwrap();
    let stored = database.load_plan(&plan.id).unwrap().unwrap();
    assert_eq!(stored.canonical_project_path, plan.project_path);
}

mod task_metadata;

mod remote_attempts;

mod task_structure;

mod qa_reconcile;

mod binding_rows;

mod task_authority;

mod local_dispatch;

mod adoption_attachments;

mod lifecycle_transactions;

mod remaining_task_writers;
