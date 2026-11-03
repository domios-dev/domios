use super::*;
use crate::plan::{CuratorRound, PlanMode, TaskStatus};
use crate::service_plan::curator::{eligible, synthesis};

#[test]
fn scheduler_preserves_complete_compare_value_eligibility_and_group_close() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Schedule".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.mode = PlanMode::Execution;
    plan.team_id = Some("team".into());
    plan.task_groups = serde_json::from_value(json!([{"key":"g","label":"Group"}])).unwrap();
    let mut done = crate::service_plan::load_audit::task("done", TaskStatus::Done, None);
    done.group = Some("g".into());
    done.status_changed_at = 17;
    plan.tasks = vec![
        done,
        crate::service_plan::load_audit::task("open", TaskStatus::Pending, None),
    ];
    plan.control_loop.rounds = (0..64)
        .map(|number| CuratorRound {
            number,
            ended_at: number,
            ..Default::default()
        })
        .collect();
    plan.strides = serde_json::from_value(json!([
        {"id":"old","from":{"tab_id":"worker"},"kind":"group_close","text":"old","ts":9},
        {"id":"latest","from":{"tab_id":"worker"},"kind":"group_close","text":"large unrelated prose","ts":21},
        {"id":"note","from":{"tab_id":"worker"},"kind":"note","text":"ignore","ts":99}
    ]))
    .unwrap();
    let database = fixtures::database(directory.path(), &plan);
    let read = database.curator_schedule_projection(&plan.id).unwrap();
    assert_eq!(read.control_loop, plan.control_loop);
    assert_eq!(synthesis::group_close(&read), Some(21));
    assert_eq!(synthesis::group_close(&read), synthesis::group_close(&plan));
    for enabled in [false, true] {
        for human_wait in [false, true] {
            assert_eq!(
                eligible(&read, enabled, human_wait),
                eligible(&plan, enabled, human_wait)
            );
        }
    }
    assert!(read.tasks.iter().all(|task| task.body.is_empty()
        && task.execution_attempts.is_empty()
        && task.milestones.is_empty()));
}

#[test]
fn scheduler_materialization_does_not_load_unrelated_history_or_bodies() {
    let mut prior = None;
    for (strides, attempts, extra_body) in [(0, 1, 0), (2048, 1, 0), (0, 2048, 0), (0, 1, 32768)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.tasks[1].body.push_str(&"x".repeat(extra_body));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let read = database.curator_schedule_projection(&plan.id).unwrap();
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        assert_eq!(read.control_loop, plan.control_loop);
        assert_eq!(read.tasks.len(), fixtures::TASKS);
        assert_eq!(metrics.queries, 3);
        assert!(metrics.materialized_bytes < 256 * 1024);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        let response_bytes=serde_json::to_vec(&json!({"eligible":eligible(&read,true,false),"group_close":synthesis::group_close(&read)})).unwrap().len();
        println!("fixture={} consumer=curator_schedule tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={extra_body} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response_bytes} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response_bytes);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}

#[test]
fn scheduler_snapshot_is_committed_and_decode_failure_is_an_error() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "A".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.tasks = vec![crate::service_plan::load_audit::task(
        "one",
        TaskStatus::Pending,
        None,
    )];
    let database = fixtures::database(directory.path(), &plan);
    let path = directory.path().join("plans.db");
    let barrier = Arc::new(Barrier::new(2));
    let writer_barrier = barrier.clone();
    let id = plan.id.clone();
    let writer = std::thread::spawn(move || {
        let mut connection = rusqlite::Connection::open(path).unwrap();
        connection
            .busy_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        for index in 0..16 {
            let mutator = connection.transaction().unwrap();
            let (title, status) = if index % 2 == 0 {
                ("B", "done")
            } else {
                ("A", "pending")
            };
            mutator
                .execute("UPDATE plans SET title=?2 WHERE id=?1", params![id, title])
                .unwrap();
            mutator.execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.status',?2) WHERE plan_id=?1",params![id,status]).unwrap();
            writer_barrier.wait();
            mutator.commit().unwrap();
            writer_barrier.wait();
        }
    });
    for _ in 0..16 {
        barrier.wait();
        let read = database.curator_schedule_projection(&plan.id).unwrap();
        assert_eq!(read.title == "B", read.tasks[0].status == TaskStatus::Done);
        barrier.wait();
    }
    writer.join().unwrap();
    database.connection.lock().execute("UPDATE plans SET header_json=json_set(header_json,'$.control_loop.mode','invalid') WHERE id=?1",[&plan.id]).unwrap();
    assert!(matches!(
        database.curator_schedule_projection(&plan.id),
        Err(PlanError::Quarantined { .. })
    ));
    assert!(matches!(
        database.curator_schedule_projection("absent"),
        Err(PlanError::NotFound)
    ));
}
