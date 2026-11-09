use super::*;
use crate::plan::{QaState, ReviewStage, TaskStatus};
use crate::service_plan::{
    qa::{qa_queue_for, qa_queue_from_facts},
    review_gate,
};

fn decorate(plan: &mut Plan) {
    plan.review_policy.wip_limit = 3;
    plan.review_policy.credit = -4;
    plan.task_groups = serde_json::from_value(json!([
        {"key":"b","label":"First B"},{"key":"a","label":"A"},{"key":"b","label":"Later B"}
    ]))
    .unwrap();
    for task in &mut plan.tasks[..3] {
        task.status = TaskStatus::InReview;
    }
    plan.tasks[0].review_stage = ReviewStage::Qa;
    plan.tasks[0].qa_state = None;
    plan.tasks[0].qa_plan = Some("QA overrides the other plan".into());
    plan.tasks[0].test_plan = Some("unused".into());
    plan.tasks[0].group = Some("b".into());
    plan.tasks[1].review_stage = ReviewStage::Qa;
    plan.tasks[1].qa_state = Some(QaState::Running);
    plan.tasks[1].qa_plan = None;
    plan.tasks[1].test_plan = Some("Fallback".into());
    plan.tasks[1].status_changed_at = u64::MAX;
    plan.tasks[1].assigned_tab_id = Some("qa-worker".into());
    plan.tasks[1].group = Some("a".into());
}

fn response(database: &super::super::super::PlanDatabase, plan: &Plan) -> serde_json::Value {
    let queue = qa_queue_from_facts(&plan.id, &database.qa_queue_tasks(&plan.id).unwrap(), 50);
    let gate = database.review_gate_projection(&plan.id).unwrap();
    let waves = review_gate::qa_waves(database.qa_wave_projection(&plan.id).unwrap());
    assert_eq!(queue, qa_queue_for(&plan.id, &plan.tasks, 50));
    assert_eq!(gate, review_gate::review_gate_status(plan));
    assert_eq!(waves, review_gate::qa_waves(plan));
    json!([queue, gate, waves])
}

#[test]
fn qa_readers_keep_queue_positions_fallback_text_review_counts_and_wave_labels() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "QA".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.tasks = (0..4)
        .map(|index| {
            crate::service_plan::load_audit::task(&format!("t{index}"), TaskStatus::Pending, None)
        })
        .collect();
    decorate(&mut plan);
    plan.tasks[2].group = Some("ignored-review-stage".into());
    let database = fixtures::database(directory.path(), &plan);
    let actual = response(&database, &plan);
    assert_eq!(actual[0]["queued"][0]["position"], 1);
    assert_eq!(actual[0]["running"][0]["age_ms"], 0);
    assert_eq!(actual[0]["running"][0]["test_plan"], "Fallback");
    assert_eq!(actual[1]["wip_count"], 3);
    assert_eq!(actual[2][0]["group_key"], "a");
    assert_eq!(actual[2][1]["label"], "First B");
    assert!(matches!(
        database.qa_queue_tasks("absent"),
        Err(PlanError::NotFound)
    ));
    assert!(matches!(
        database.review_gate_projection("absent"),
        Err(PlanError::NotFound)
    ));
    assert!(matches!(
        database.qa_wave_projection("absent"),
        Err(PlanError::NotFound)
    ));
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.qa_plan',json('{}')) WHERE plan_id=?1 AND task_id='t0'",[&plan.id]).unwrap();
    assert!(matches!(
        database.qa_queue_tasks(&plan.id),
        Err(PlanError::Quarantined { .. })
    ));
    database.connection.lock().execute("UPDATE plans SET header_json=json_set(header_json,'$.review_policy.credit',NULL,'$.task_groups[0].label',NULL) WHERE id=?1",[&plan.id]).unwrap();
    assert!(matches!(
        database.review_gate_projection(&plan.id),
        Err(PlanError::Quarantined { .. })
    ));
    assert!(matches!(
        database.qa_wave_projection(&plan.id),
        Err(PlanError::Quarantined { .. })
    ));
}

#[test]
fn qa_and_review_readers_ignore_unrelated_history_bodies_and_unused_test_plan_text() {
    let mut prior = None;
    for (strides, attempts, extra) in [(0, 1, 0), (2048, 1, 0), (0, 2048, 0), (0, 1, 32768)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        decorate(&mut plan);
        plan.tasks[3].body.push_str(&"x".repeat(extra));
        plan.tasks[0].test_plan = Some("x".repeat(extra));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        super::super::super::super::store_sync::take_decoded_row_bytes();
        let full = database.load_plan(&plan.id).unwrap().unwrap();
        let baseline = super::super::super::super::store_sync::take_decoded_row_bytes();
        take_metrics();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let output = response(&database, &full);
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        let response_bytes = serde_json::to_vec(&output).unwrap().len();
        assert_eq!(metrics.queries, 5);
        assert!(metrics.materialized_bytes < 4096);
        assert!(decoded < 4096);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=qa_review tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={extra} baseline_decoded_json_bytes={baseline} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response_bytes} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response_bytes);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior)
        }
        prior = Some(measured);
    }
}

#[test]
fn qa_waves_keep_task_group_and_label_from_the_same_committed_transaction() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "QA".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    let mut task = crate::service_plan::load_audit::task("task", TaskStatus::InReview, None);
    task.review_stage = ReviewStage::Qa;
    task.group = Some("A".into());
    plan.tasks = vec![task];
    plan.task_groups =
        serde_json::from_value(json!([{"key":"A","label":"A"},{"key":"B","label":"A"}])).unwrap();
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
            let key = if index % 2 == 0 { "B" } else { "A" };
            let tx = connection.transaction().unwrap();
            tx.execute("UPDATE plans SET header_json=json_set(header_json,'$.task_groups[0].label',?2,'$.task_groups[1].label',?2) WHERE id=?1",params![id,key]).unwrap();
            tx.execute(
                "UPDATE plan_tasks SET row_json=json_set(row_json,'$.group',?2) WHERE plan_id=?1",
                params![id, key],
            )
            .unwrap();
            writer_barrier.wait();
            tx.commit().unwrap();
            writer_barrier.wait();
        }
    });
    for _ in 0..16 {
        barrier.wait();
        let projected = database.qa_wave_projection(&plan.id).unwrap();
        let wave = review_gate::qa_waves(projected).remove(0);
        assert_eq!(wave.group_key, wave.label);
        barrier.wait();
    }
    writer.join().unwrap();
}
