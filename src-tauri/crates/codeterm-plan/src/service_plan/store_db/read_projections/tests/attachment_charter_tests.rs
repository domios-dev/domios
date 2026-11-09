use super::*;
use crate::plan::{PlanMode, TaskStatus};
use crate::service_plan::{paths, stride_context, worker_charter, worker_identity};

fn milestone(id: &str, kind: &str, text: &str, ts: u64) -> crate::plan::Milestone {
    serde_json::from_value(
        json!({"id":id,"kind":kind,"text":text,"ts":ts,"from":{"tab_id":"owner"}}),
    )
    .unwrap()
}

fn decorate(plan: &mut Plan) {
    plan.mode = PlanMode::Execution;
    plan.owner = OrchestratorRef::from_tab("owner");
    plan.sections=serde_json::from_value(json!([{"id":"sec_root","title":"Root","body":"unreturned section text","subsections":[{"id":"sec_child","title":"Child"}]}])).unwrap();
    let task = &mut plan.tasks[0];
    task.assigned_tab_id = Some("worker".into());
    task.personality = Some("Fixture persona".into());
    task.role_guidance = Some("Opaque guidance".into());
    task.milestones = (0..6)
        .map(|i| {
            milestone(
                &format!("chosen-{i}"),
                "decision",
                &format!("Chosen decision {i}"),
                10 + i,
            )
        })
        .collect();
    plan.loops=serde_json::from_value(json!([
        {"id":"stopped","target":task.id,"budget":5,"count":2,"status":"stopped","stagnation":0},
        {"id":"selected","target":format!("\u{2003}{}\u{2003}",task.id),"budget":5,"count":2,"status":"running","stagnation":0,"stop_when":"QA passes"},
        {"id":"later","target":task.id,"budget":8,"count":4,"status":"running","stagnation":0}
    ])).unwrap();
    let mut retired = crate::service_plan::load_audit::attempt("retired-open", "other-worker");
    retired.retired_at = Some(2);
    task.execution_attempts.push(retired);
}

fn expected_charter(plan: &Plan) -> String {
    let task = &plan.tasks[0];
    let identity = worker_identity::compose_worker_identity(
        task.personality.as_deref(),
        task.role_guidance.as_deref(),
        Some(&task.name),
    );
    let mut body = worker_charter::render_worker_charter(
        &task.id,
        identity.as_deref(),
        Some(&worker_charter::LoopCharterRead::from(&plan.loops[1])),
        crate::service_plan::task_attempts::live_attempt_count(task),
        0,
    );
    if let Some(strides) = stride_context::render_worker_strides(&task.milestones) {
        body = format!("{body}\n\n{strides}")
    }
    format!("{body}\n\ncontinue")
}

#[test]
fn attachment_reference_snapshot_preserves_nested_ids_and_committed_deletion_boundary() {
    let directory = tempfile::tempdir().unwrap();
    paths::mark_test_project_root(directory.path());
    let root = paths::project_root(directory.path());
    let mut plan = Plan::new(
        "Refs".into(),
        root.display().to_string(),
        OrchestratorRef::default(),
    );
    plan.tasks = vec![crate::service_plan::load_audit::task(
        "task",
        TaskStatus::Pending,
        None,
    )];
    decorate(&mut plan);
    let database = fixtures::database(directory.path(), &plan);
    let candidates = [
        ("tasks", "task"),
        ("sections", "sec_child"),
        ("milestones", "chosen-0"),
        ("milestones", "removed"),
    ];
    let read = database.attachment_reference_read().unwrap();
    assert_eq!(read.project_plan_ids(&root).unwrap(), vec![plan.id.clone()]);
    database
        .connection
        .lock()
        .execute("DELETE FROM plan_tasks WHERE plan_id=?1", [&plan.id])
        .unwrap();
    database
        .connection
        .lock()
        .execute("DELETE FROM plan_sections WHERE plan_id=?1", [&plan.id])
        .unwrap();
    assert_eq!(
        read.known_directories(&plan.id, &candidates).unwrap(),
        vec![true, true, true, false]
    );
    read.finish().unwrap();
    let read = database.attachment_reference_read().unwrap();
    assert_eq!(
        read.known_directories(&plan.id, &candidates).unwrap(),
        vec![false, false, false, false]
    );
    read.finish().unwrap();
    database.persist_plan_rows(&plan, None).unwrap();
    database.connection.lock().execute("UPDATE plan_task_milestones SET row_json=json_set(row_json,'$.id',NULL) WHERE plan_id=?1",[&plan.id]).unwrap();
    let read = database.attachment_reference_read().unwrap();
    assert!(matches!(
        read.known_directories(&plan.id, &candidates),
        Err(PlanError::Quarantined { .. })
    ));
}

#[test]
fn worker_charter_preserves_owner_priority_mode_task_order_lane_and_retired_attempt_bindings() {
    let directory = tempfile::tempdir().unwrap();
    paths::mark_test_project_root(directory.path());
    let mut first = Plan::new(
        "First".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    first.id = "pl_a".into();
    first.updated_at = 10;
    first.tasks = vec![crate::service_plan::load_audit::task(
        "first",
        TaskStatus::Pending,
        None,
    )];
    decorate(&mut first);
    let mut second = first.clone();
    second.id = "pl_b".into();
    second.tasks[0].id = "second".into();
    let mut owned = first.clone();
    owned.id = "pl_owned".into();
    owned.updated_at = 20;
    owned.owner = OrchestratorRef::from_tab("worker");
    owned.tasks[0].assigned_tab_id = None;
    let database = fixtures::database(directory.path(), &first);
    database.persist_plan_rows(&second, None).unwrap();
    database.persist_plan_rows(&owned, None).unwrap();
    assert_eq!(
        database.active_owned_open_plan("worker").unwrap(),
        Some(owned.id.clone())
    );
    assert!(database
        .worker_assignment_projection("worker")
        .unwrap()
        .is_none());
    owned.tasks[0].status = TaskStatus::Paused;
    database.persist_plan_rows(&owned, None).unwrap();
    assert!(database.active_owned_open_plan("worker").unwrap().is_none());
    assert_eq!(
        database
            .worker_assignment_projection("worker")
            .unwrap()
            .unwrap()
            .task_id,
        "first"
    );
    first.mode = PlanMode::Planning;
    database.persist_plan_rows(&first, None).unwrap();
    assert_eq!(
        database
            .worker_assignment_projection("worker")
            .unwrap()
            .unwrap()
            .task_id,
        "second"
    );
    second.tasks[0].status = TaskStatus::InReview;
    database.persist_plan_rows(&second, None).unwrap();
    assert!(database
        .worker_assignment_projection("worker")
        .unwrap()
        .is_none());
    second.tasks[0].status = TaskStatus::Blocked;
    second.tasks[0].assigned_tab_id = None;
    second.tasks[0].lane.tab_id = Some("worker".into());
    database.persist_plan_rows(&second, None).unwrap();
    assert_eq!(
        database
            .worker_assignment_projection("worker")
            .unwrap()
            .unwrap()
            .task_id,
        "second"
    );
    second.tasks[0].lane.tab_id = None;
    second.tasks[0].execution_attempts[0].executor_tab_id = "worker".into();
    second.tasks[0].execution_attempts[0].retired_at = Some(1);
    database.persist_plan_rows(&second, None).unwrap();
    assert_eq!(
        database
            .worker_assignment_projection("worker")
            .unwrap()
            .unwrap()
            .task_id,
        "second"
    );
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.role_guidance',42) WHERE plan_id=?1",[&second.id]).unwrap();
    assert!(matches!(
        database.worker_assignment_projection("worker"),
        Err(PlanError::Quarantined { .. })
    ));
    second.tasks[0].id = "42".into();
    second.loops[1].target = "42".into();
    database.persist_plan_rows(&second, None).unwrap();
    database
        .connection
        .lock()
        .execute(
            "UPDATE plans SET header_json=json_set(header_json,'$.loops[1].target',42) WHERE id=?1",
            [&second.id],
        )
        .unwrap();
    assert!(matches!(
        database.worker_assignment_projection("worker"),
        Err(PlanError::Quarantined { .. })
    ));
    owned.tasks[0].status = TaskStatus::Pending;
    database.persist_plan_rows(&owned, None).unwrap();
    database
        .connection
        .lock()
        .execute(
            "UPDATE plans SET header_json=json_set(header_json,'$.owner.tab_id',42) WHERE id=?1",
            [&owned.id],
        )
        .unwrap();
    assert!(matches!(
        database.active_owned_open_plan("42"),
        Err(PlanError::Quarantined { .. })
    ));
}

#[test]
fn attachment_charter_and_owner_reads_ignore_unrelated_ids_attempt_history_and_body_growth() {
    let mut prior = None;
    for (strides, attempts, extra, history) in [
        (0, 1, 0, 0),
        (2048, 1, 0, 0),
        (0, 2048, 0, 0),
        (0, 1, 32768, 0),
        (0, 1, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        paths::mark_test_project_root(directory.path());
        let root = paths::project_root(directory.path());
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.project_path = root.display().to_string();
        plan.canonical_project_path = plan.project_path.clone();
        decorate(&mut plan);
        plan.tasks[0].body.push_str(&"x".repeat(extra));
        plan.sections[0].body.push_str(&"x".repeat(extra));
        plan.loops[1].deviation = Some("x".repeat(extra));
        for i in 0..history {
            plan.tasks[0].milestones.push(milestone(
                &format!("unselected-{i}"),
                "note",
                "Old irrelevant history",
                1,
            ));
            let mut closed =
                crate::service_plan::load_audit::attempt(&format!("closed-{i}"), "unrelated");
            closed.closed_at = Some(1);
            plan.tasks[0].execution_attempts.push(closed);
        }
        let database = fixtures::database(directory.path(), &plan);
        let store = PlanStore::open(database.path()).unwrap();
        let data = store.plan_data_dir(&plan.id).join("attachments");
        for (kind, id) in [
            ("tasks", plan.tasks[0].id.as_str()),
            ("tasks", "removed_task"),
            ("sections", "sec_child"),
            ("sections", "removed_section"),
            ("milestones", "chosen-0"),
            ("milestones", "history-0"),
        ] {
            std::fs::create_dir_all(data.join(kind).join(id)).unwrap();
        }
        fixtures::audit(&database);
        super::super::super::super::store_sync::take_decoded_row_bytes();
        let full = database.load_plan(&plan.id).unwrap().unwrap();
        let baseline = super::super::super::super::store_sync::take_decoded_row_bytes();
        take_metrics();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let orphans = store.orphan_attachment_dirs(&root).unwrap();
        let mut names = orphans
            .iter()
            .map(|entry| {
                entry
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            })
            .collect::<Vec<_>>();
        names.sort();
        assert_eq!(names, vec!["history-0", "removed_section", "removed_task"]);
        let owner = store
            .active_owned_plan_with_open_work_for_tab("owner")
            .unwrap();
        assert_eq!(owner, Some(plan.id.clone()));
        let charter = store
            .prepend_worker_charter_for_tab("worker", "continue")
            .unwrap();
        assert_eq!(charter, expected_charter(&full));
        assert!(store.plan_loads_for_test().is_empty());
        let response_bytes = serde_json::to_vec(&json!([names, owner, charter]))
            .unwrap()
            .len();
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        assert_eq!(metrics.queries, 8);
        assert!(metrics.materialized_bytes < 16384);
        assert!(decoded < 16384);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=attachment_charter tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={extra} unselected_target_history={history} baseline_decoded_json_bytes={baseline} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response_bytes} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response_bytes);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior)
        }
        prior = Some(measured);
    }
}

#[test]
fn worker_charter_reads_task_identity_and_loop_stop_text_in_one_commit() {
    let directory = tempfile::tempdir().unwrap();
    paths::mark_test_project_root(directory.path());
    let mut plan = Plan::new(
        "Race".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.tasks = vec![crate::service_plan::load_audit::task(
        "task",
        TaskStatus::Pending,
        None,
    )];
    decorate(&mut plan);
    plan.tasks[0].role_guidance = Some("A".into());
    plan.loops[1].stop_when = Some("A".into());
    let database = fixtures::database(directory.path(), &plan);
    let path = database.path();
    let id = plan.id.clone();
    let barrier = Arc::new(Barrier::new(2));
    let writer_barrier = barrier.clone();
    let writer = std::thread::spawn(move || {
        let mut connection = rusqlite::Connection::open(path).unwrap();
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        for index in 0..16 {
            let value = if index % 2 == 0 { "B" } else { "A" };
            let tx = connection.transaction().unwrap();
            tx.execute("UPDATE plans SET header_json=json_set(header_json,'$.loops[1].stop_when',?2) WHERE id=?1",params![id,value]).unwrap();
            tx.execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.role_guidance',?2) WHERE plan_id=?1",params![id,value]).unwrap();
            writer_barrier.wait();
            tx.commit().unwrap();
            writer_barrier.wait();
        }
    });
    for _ in 0..16 {
        barrier.wait();
        let read = database
            .worker_assignment_projection("worker")
            .unwrap()
            .unwrap();
        assert_eq!(read.role_guidance, read.loop_spec.unwrap().stop_when);
        barrier.wait();
    }
    writer.join().unwrap();
}

#[test]
fn unrelated_worker_charter_binding_is_indexed_and_materializes_no_task_rows() {
    for (strides, attempts, extra) in [(0, 1, 0), (2048, 1, 0), (0, 2048, 0), (0, 1, 32768)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.tasks[0].body.push_str(&"x".repeat(extra));
        let database = fixtures::database(directory.path(), &plan);
        let details = {
            let connection = database.connection.lock();
            let mut statement = connection
                .prepare(&format!(
                    "EXPLAIN QUERY PLAN {}",
                    crate::service_plan::store_db::charter_queries::WORKER_ASSIGNMENT_SQL
                ))
                .unwrap();
            let rows = statement
                .query_map(
                    params![
                        "unrelated-worker",
                        crate::service_plan::store_db::status_queries::WHITESPACE
                    ],
                    |row| row.get::<_, String>(3),
                )
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            rows
        };
        for index in [
            "plan_tasks_assigned_tab_id",
            "plan_tasks_lane_tab_id",
            "plan_task_attempts_executor_tab_id",
        ] {
            assert!(
                details
                    .iter()
                    .any(|detail| detail.contains("SEARCH") && detail.contains(index)),
                "{index}: {details:?}"
            );
        }
        fixtures::audit(&database);
        take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        assert!(database
            .worker_assignment_projection("unrelated-worker")
            .unwrap()
            .is_none());
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        assert_eq!(metrics.queries, 2);
        assert_eq!(metrics.materialized_bytes, 0);
        assert_eq!(decoded, 0);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=unrelated_worker_charter tasks={} old_strides={strides} unrelated_attempts={attempts} extra_body={extra} queries=2 materialized_text_bytes=0 decoded_json_bytes=0 serialized_row_bytes=0 response_bytes=0 changed_rows=0",fixtures::SEED,fixtures::TASKS);
    }
}
