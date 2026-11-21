use super::*;
use crate::plan::{PlanStatus, TaskStatus};

#[test]
fn team_close_facts_exclude_unrelated_history_and_deduplicate_lane_attempts() {
    let mut previous = None;
    for (strides, attempts, body, duplicates) in [
        (0, 1, 0, 1),
        (2048, 1, 0, 1),
        (0, 2048, 0, 1),
        (0, 1, 32768, 1),
        (0, 1, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.team_id = Some("closing".into());
        for task in &mut plan.tasks {
            task.status = TaskStatus::Done;
        }
        let task = &mut plan.tasks[0];
        task.status = TaskStatus::InProgress;
        task.assigned_tab_id = Some("member".into());
        task.lane.branch = Some("lane/member".into());
        task.body.push_str(&"x".repeat(body));
        let current = task.execution_attempts[0].clone();
        task.execution_attempts = (0..duplicates)
            .map(|index| {
                let mut attempt = current.clone();
                attempt.id = format!("attempt-{index}");
                attempt
            })
            .collect();
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let rows = database
            .team_close_projection("closing", &["member".into()])
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].tasks.len(), 1);
        assert_eq!(rows[0].tasks[0].member_executor.as_deref(), Some("member"));
        assert_eq!(rows[0].tasks[0].attempt_lanes.len(), 1);
        assert_eq!(
            rows[0].tasks[0].attempt_lanes[0].branch.as_deref(),
            Some("lane/member")
        );
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&rows).unwrap().len();
        assert_eq!(metrics.queries, 2);
        assert!(metrics.materialized_bytes < 4096);
        assert!(decoded < 4096);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=team_close tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} selected_extra_body={body} duplicate_lanes={duplicates} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(previous) = &previous {
            assert_eq!(&measured, previous);
        }
        previous = Some(measured);
    }
}

#[test]
fn team_close_executor_precedence_remote_exclusion_and_selected_errors() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Closing".into(),
        directory.path().display().to_string(),
        Default::default(),
    );
    plan.team_id = Some("closing".into());
    let mut task =
        crate::service_plan::load_audit::task("task", TaskStatus::InProgress, Some("assigned"));
    task.lane.tab_id = Some("lane".into());
    task.lane.branch = Some("lane/test".into());
    let mut closed = crate::service_plan::load_audit::attempt("closed", "closed");
    closed.closed_at = Some(1);
    let mut retired = crate::service_plan::load_audit::attempt("retired", "retired");
    retired.retired_at = Some(1);
    retired.remote_worktree_path = Some("remote".into());
    task.execution_attempts = vec![closed, retired];
    plan.tasks = vec![task];
    let database = fixtures::database(directory.path(), &plan);
    for (members, expected) in [
        (
            vec!["assigned".into(), "lane".into(), "retired".into()],
            Some("assigned"),
        ),
        (vec!["lane".into(), "retired".into()], Some("lane")),
        (vec!["closed".into(), "retired".into()], Some("retired")),
        (vec!["closed".into()], None),
    ] {
        let rows = database.team_close_projection("closing", &members).unwrap();
        assert_eq!(rows[0].tasks[0].member_executor.as_deref(), expected);
        assert!(rows[0].tasks[0].has_remote_attempt);
        assert!(rows[0].tasks[0].attempt_lanes.is_empty());
    }
    database
        .connection
        .lock()
        .execute(
            "UPDATE plan_tasks SET row_json=json_set(row_json,'$.name',17) WHERE plan_id=?1",
            [&plan.id],
        )
        .unwrap();
    assert!(matches!(
        database.team_close_projection("closing", &[]),
        Err(PlanError::Quarantined { .. })
    ));
}

#[test]
fn team_close_headers_and_selected_task_facts_share_one_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Closing".into(),
        directory.path().display().to_string(),
        Default::default(),
    );
    plan.team_id = Some("closing".into());
    let mut task = crate::service_plan::load_audit::task("task", TaskStatus::InProgress, None);
    task.name = "active".into();
    task.lane.branch = Some("lane/test".into());
    plan.tasks = vec![task];
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
            let status = if index % 2 == 0 { "archived" } else { "active" };
            let tx = connection.transaction().unwrap();
            tx.execute("UPDATE plans SET status=?2,header_json=json_set(header_json,'$.status',?2) WHERE id=?1",params![id,status]).unwrap();
            tx.execute(
                "UPDATE plan_tasks SET row_json=json_set(row_json,'$.name',?2) WHERE plan_id=?1",
                params![id, status],
            )
            .unwrap();
            writer_barrier.wait();
            tx.commit().unwrap();
            writer_barrier.wait();
        }
    });
    for _ in 0..16 {
        barrier.wait();
        let rows = database.team_close_projection("closing", &[]).unwrap();
        assert_eq!(rows[0].tasks.len(), 1);
        assert_eq!(
            rows[0].tasks[0].name,
            if rows[0].status == PlanStatus::Active {
                "active"
            } else {
                "archived"
            }
        );
        barrier.wait();
    }
    writer.join().unwrap();
}

#[test]
fn team_close_projection_bounded_ignores_unrelated_plans_and_history_and_loads_open_attempts() {
    let directory = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(directory.path());
    let mut selected_task =
        crate::service_plan::load_audit::task("task-open", TaskStatus::InProgress, Some("member"));
    selected_task.name = "Active work".into();
    selected_task.body = "b".repeat(16_384);
    let local_open = crate::service_plan::load_audit::attempt("att-local", "member");
    let mut remote_open = crate::service_plan::load_audit::attempt("att-remote", "remote-tab");
    remote_open.remote_worktree_path = Some("/remote/worktree".into());
    selected_task.execution_attempts.push(local_open);
    selected_task.execution_attempts.push(remote_open);
    for index in 0..256 {
        let mut closed = crate::service_plan::load_audit::attempt(&format!("closed-{index}"), "member");
        closed.closed_at = Some(index as u64 + 1);
        selected_task.execution_attempts.push(closed);
    }
    let mut tasks = vec![selected_task];
    for index in 0..256 {
        let mut done = crate::service_plan::load_audit::task(
            &format!("done-{index}"),
            TaskStatus::Done,
            Some("member"),
        );
        done.body = "h".repeat(1024);
        tasks.push(done);
    }
    let mut closing = crate::service_plan::load_audit::execution_plan(
        directory.path(),
        "Closing Plan Title",
        "owner",
        tasks,
    );
    closing.team_id = Some("closing".into());
    let closing_id = crate::service_plan::load_audit::seed(&store, closing);

    let unrelated_tasks = (0..128)
        .map(|index| {
            crate::service_plan::load_audit::task(
                &format!("other-{index}"),
                TaskStatus::InProgress,
                Some("other-member"),
            )
        })
        .collect();
    let mut unrelated = crate::service_plan::load_audit::execution_plan(
        directory.path(),
        "Unrelated Plan",
        "other-owner",
        unrelated_tasks,
    );
    unrelated.team_id = Some("other-team".into());
    let unrelated_id = crate::service_plan::load_audit::seed(&store, unrelated);

    crate::service_plan::load_audit::start(&store);
    take_metrics();
    super::super::super::super::store_sync::take_decoded_row_bytes();
    let rows = store
        .team_close_projection_bounded("closing", &["member".into()], 32)
        .unwrap();
    let metrics = take_metrics();
    let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, closing_id);
    assert_eq!(rows[0].title, "Closing Plan Title");
    assert_eq!(rows[0].tasks.len(), 1);
    assert_eq!(rows[0].tasks[0].id, "task-open");
    assert_eq!(rows[0].tasks[0].open_attempts.len(), 2);
    assert_eq!(rows[0].tasks[0].open_attempts[0].id, "att-local");
    assert!(!rows[0].tasks[0].open_attempts[0].remote);
    assert_eq!(rows[0].tasks[0].open_attempts[1].id, "att-remote");
    assert!(rows[0].tasks[0].open_attempts[1].remote);
    assert_eq!(metrics.queries, 3);
    assert!(decoded < 8192);
    crate::service_plan::load_audit::assert_never_loaded(&store, &[&closing_id, &unrelated_id]);
}

#[test]
fn team_close_projection_bounded_refuses_atomically_when_selected_rows_exceed_cap() {
    let directory = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(directory.path());
    let tasks = (0..5)
        .map(|index| {
            let mut task = crate::service_plan::load_audit::task(
                &format!("t-{index}"),
                TaskStatus::InProgress,
                Some("member"),
            );
            task.execution_attempts.push(crate::service_plan::load_audit::attempt(
                &format!("a-{index}-1"),
                "member",
            ));
            task.execution_attempts.push(crate::service_plan::load_audit::attempt(
                &format!("a-{index}-2"),
                "member",
            ));
            task
        })
        .collect();
    let mut plan = crate::service_plan::load_audit::execution_plan(
        directory.path(),
        "Bounded Plan",
        "owner",
        tasks,
    );
    plan.team_id = Some("closing".into());
    crate::service_plan::load_audit::seed(&store, plan);

    for cap in [0, 4, 10, 15] {
        assert!(
            matches!(
                store.team_close_projection_bounded("closing", &["member".into()], cap),
                Err(crate::service_plan::TeamCloseProjectionError::TooLarge)
            ),
            "cap {cap} must refuse atomically"
        );
    }
    let exact = store
        .team_close_projection_bounded("closing", &["member".into()], 16)
        .unwrap();
    assert_eq!(exact.len(), 1);
    assert_eq!(exact[0].tasks.len(), 5);
    assert_eq!(
        exact[0]
            .tasks
            .iter()
            .map(|task| task.open_attempts.len())
            .sum::<usize>(),
        10
    );
}
