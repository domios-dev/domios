use super::*;
use crate::plan::{PlanStatus, TaskStatus};

fn elements() -> Vec<crate::plan::CanvasElement> {
    serde_json::from_value(json!([
        {"id":"same","kind":"handoff","content":{"seq":1,"reseed_state":"new_sent","reseed_pending":{"tab_id":"worker","source_session_id":"new","state":"new_sent"}}},
        {"id":"first","kind":"handoff","content":{"seq":7,"reseed_state":"new_sent","reseed_pending":{"tab_id":"worker","source_session_id":"old","state":"session_seen"}}},
        {"id":"second","kind":"handoff","content":{"seq":3,"reseed_state":"new_sent","reseed_pending":{"tab_id":"worker","source_session_id":"old","state":"new_sent"}}},
        {"id":"seen","kind":"handoff","content":{"seq":2,"reseed_state":"session_seen","reseed_pending":{"tab_id":"other","source_session_id":17}}}
    ])).unwrap()
}

#[test]
fn reseed_selectors_keep_archived_visibility_plan_order_and_first_matching_element() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(root.path(), 0, 1);
    plan.id = "a".into();
    plan.updated_at = 10;
    plan.status = PlanStatus::Archived;
    plan.elements = elements();
    let database = fixtures::database(root.path(), &plan);
    let mut newer = plan.clone();
    newer.id = "z".into();
    newer.updated_at = 20;
    database.persist_plan(&newer, None).unwrap();
    assert!(database.active_reseed_for_tab("worker", None).unwrap());
    assert!(database.active_reseed_for_tab("other", Some("z")).unwrap());
    assert!(!database.active_reseed_for_tab("missing", None).unwrap());
    let rows = database.handoff_resume_candidates("worker", "new").unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| (row.plan_id.as_str(), row.handoff_id.as_str()))
            .collect::<Vec<_>>(),
        [("z", "first"), ("a", "first")]
    );
    assert!(database
        .handoff_resume_candidates("other", "new")
        .unwrap()
        .is_empty());
    let rows = database.handoff_resume_candidates("worker", "old").unwrap();
    assert!(rows.iter().all(|row| row.handoff_id == "same"));
    assert_eq!(database.handoff_create_input("a", true).unwrap().seq, 8);
}

#[test]
fn handoff_inputs_exclude_task_bodies_attempts_and_unselected_history() {
    let mut previous = None;
    for (strides, attempts, body) in [(0, 1, 0), (2048, 1, 0), (0, 2048, 0), (0, 1, 32768)] {
        let root = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(root.path(), strides, attempts);
        for task in &mut plan.tasks {
            task.status = TaskStatus::Done;
        }
        plan.tasks[0].status = TaskStatus::InProgress;
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.elements = elements();
        plan.elements[0].content["unused"] = json!("x".repeat(body));
        plan.tasks[0].milestones = serde_json::from_value(
            json!([{"id":"decision","kind":"decision","text":"kept","ts":1,"from":{}}]),
        )
        .unwrap();
        let database = fixtures::database(root.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let input = database.handoff_create_input(&plan.id, true).unwrap();
        assert!(database.active_reseed_for_tab("worker", None).unwrap());
        let candidates = database.handoff_resume_candidates("worker", "new").unwrap();
        assert_eq!(input.total, fixtures::TASKS);
        assert_eq!(input.done, fixtures::TASKS - 1);
        assert_eq!(input.seq, 8);
        assert_eq!(input.in_flight.len(), 1);
        assert_eq!(input.decisions, ["kept"]);
        assert_eq!(candidates.len(), 1);
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        assert_eq!(metrics.queries, 5);
        assert!(metrics.materialized_bytes < 2048);
        assert!(decoded < 2048);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        let response = serde_json::to_vec(
            &json!({"tasks":input.in_flight,"decisions":input.decisions,"seq":input.seq}),
        )
        .unwrap()
        .len();
        println!("fixture={} consumer=handoff_readers tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} selected_extra_body={body} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(previous) = &previous {
            assert_eq!(&measured, previous);
        }
        previous = Some(measured);
    }
}

#[test]
fn handoff_selected_errors_and_sequence_exhaustion_fail_before_writing() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(root.path(), 0, 1);
    plan.elements = elements();
    let database = fixtures::database(root.path(), &plan);
    assert!(matches!(
        database.handoff_create_input("missing", true),
        Err(PlanError::NotFound)
    ));
    database.connection.lock().execute("UPDATE plan_elements SET row_json=json_set(row_json,'$.content.seq',json(?2)) WHERE plan_id=?1 AND position=0",params![plan.id,u64::MAX.to_string()]).unwrap();
    assert!(matches!(
        database.handoff_create_input(&plan.id, true),
        Err(PlanError::Io(_))
    ));
    database.connection.lock().execute("UPDATE plan_elements SET row_json=json_set(row_json,'$.content.seq',1) WHERE plan_id=?1 AND position=0",[&plan.id]).unwrap();
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.handle',17) WHERE plan_id=?1 AND task_id='task_0000'",[&plan.id]).unwrap();
    fixtures::audit(&database);
    assert!(matches!(
        database.handoff_create_input(&plan.id, false),
        Err(PlanError::Quarantined { .. })
    ));
    assert!(fixtures::audit_counts(&database).is_empty());
}

#[test]
fn handoff_scope_task_and_decision_facts_share_the_committed_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "A".into(),
        root.path().display().to_string(),
        Default::default(),
    );
    let mut task = crate::service_plan::load_audit::task("task", TaskStatus::InProgress, None);
    task.handle = Some("A".into());
    task.milestones = serde_json::from_value(
        json!([{"id":"decision","kind":"decision","text":"A","ts":1,"from":{}}]),
    )
    .unwrap();
    plan.tasks = vec![task];
    let database = fixtures::database(root.path(), &plan);
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
            let transaction = connection.transaction().unwrap();
            transaction.execute("UPDATE plans SET title=?2,header_json=json_set(header_json,'$.title',?2) WHERE id=?1",params![id,value]).unwrap();
            transaction.execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.handle',?2) WHERE plan_id=?1",params![id,value]).unwrap();
            transaction.execute("UPDATE plan_task_milestones SET row_json=json_set(row_json,'$.text',?2) WHERE plan_id=?1",params![id,value]).unwrap();
            writer_barrier.wait();
            transaction.commit().unwrap();
            writer_barrier.wait();
        }
    });
    for _ in 0..16 {
        barrier.wait();
        let read = database.handoff_create_input(&plan.id, true).unwrap();
        assert_eq!(read.title, read.in_flight[0].task_handle);
        assert_eq!(read.title, read.decisions[0]);
        barrier.wait();
    }
    writer.join().unwrap();
}
