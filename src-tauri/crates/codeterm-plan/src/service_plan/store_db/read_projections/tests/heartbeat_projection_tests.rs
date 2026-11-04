use super::*;
use crate::plan::{PlanMode, PlanOrphan, TaskStatus};
use crate::service_plan::heartbeat::view::HeartbeatView;

#[test]
fn heartbeat_sql_preserves_actions_sections_and_selected_learning_order() {
    let directory = tempfile::tempdir().unwrap();
    let base = super::curator_notice_tests::decisions(directory.path());
    let selections: [Option<Vec<String>>; 5] = [
        None,
        Some(vec![]),
        Some(vec![
            "fourth".into(),
            "first".into(),
            "third".into(),
            "second".into(),
        ]),
        Some(vec![
            "empty".into(),
            "first".into(),
            "missing".into(),
            "second".into(),
            "third".into(),
        ]),
        Some(vec![
            "third".into(),
            "third".into(),
            "first".into(),
            "second".into(),
        ]),
    ];
    for mode in [
        PlanMode::Planning,
        PlanMode::Approved,
        PlanMode::Execution,
        PlanMode::Paused,
        PlanMode::Complete,
    ] {
        for state in 0..3 {
            let case = tempfile::tempdir().unwrap();
            let mut plan = base.clone();
            plan.mode = mode.clone();
            if state == 1 {
                for task in &mut plan.tasks {
                    task.status = TaskStatus::Done;
                }
            }
            if state == 2 {
                plan.team_id = None;
                plan.orphaned = Some(PlanOrphan {
                    team_id: "old".into(),
                    reason: "closed".into(),
                    unfinished_task_ids: vec![],
                    at: 1,
                });
            }
            let database = fixtures::database(case.path(), &plan);
            let digests = vec!["opaque digest".into(), "  ".into()];
            for selection in &selections {
                let read = database
                    .heartbeat_projection(&plan.id, 10_000, selection.as_deref())
                    .unwrap();
                assert_eq!(
                    read.prepend("opaque payload", &digests),
                    HeartbeatView::from_plan_at(&plan, 10_000).prepend(
                        "opaque payload",
                        &digests,
                        selection.as_deref()
                    ),
                    "mode={mode:?} state={state} selection={selection:?}"
                );
            }
        }
    }
}

#[test]
fn heartbeat_materialization_is_output_sized_and_excludes_unrelated_history() {
    let mut previous = None;
    for (strides, attempts, body, learning_tail) in [
        (0, 1, 0, 0),
        (2048, 1, 0, 0),
        (0, 2048, 0, 0),
        (0, 1, 32768, 0),
        (0, 1, 0, 32768),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.mode = PlanMode::Execution;
        plan.tasks[1].body.push_str(&"x".repeat(body));
        plan.heartbeat.learnings=serde_json::from_value(json!([{"id":"rule","text":"guard   text","tag":"owner","ts":1},{"id":"note","text":format!("{}{}","α".repeat(500),"x".repeat(learning_tail)),"ts":2}])).unwrap();
        let database = fixtures::database(directory.path(), &plan);
        let store = PlanStore::open(database.path()).unwrap();
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let response = store.prepend_heartbeat_for_plan(&plan.id, "fixed").unwrap();
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        assert_eq!(
            response,
            HeartbeatView::from_plan_at(&plan, 10_000).prepend("fixed", &[], None)
        );
        assert!(response.contains("- guard text\n"));
        assert!(response.contains(&format!("- note: {}…\n", "α".repeat(160))));
        assert!(metrics.materialized_bytes < response.len() + 8192);
        assert!(decoded < 8192);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        assert!(store.plan_loads_for_test().is_empty());
        println!("fixture={} consumer=heartbeat_projection tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} selected_learning_tail={learning_tail} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes,response.len());
        let measured = (metrics, decoded, response.len());
        if let Some(previous) = &previous {
            assert_eq!(&measured, previous);
        }
        previous = Some(measured);
    }
}

#[test]
fn heartbeat_output_has_no_curator_row_cap_and_selected_failures_are_errors() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Heartbeat".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.mode = PlanMode::Execution;
    plan.tasks = vec![crate::service_plan::load_audit::task(
        "pending",
        TaskStatus::Pending,
        None,
    )];
    plan.loops=(0..4098).map(|index|serde_json::from_value(json!({"id":format!("loop-{index}"),"target":"fixed","status":"running","count":1,"budget":1})).unwrap()).collect();
    let database = fixtures::database(directory.path(), &plan);
    let read = database
        .heartbeat_projection(&plan.id, 10_000, None)
        .unwrap();
    let response = read.prepend("fixed", &[]);
    assert_eq!(response.matches("consider stop: loop ").count(), 4098);
    assert!(response.contains("loop loop-4097 nearly exhausted"));
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.interruption.handoff_required',2) WHERE plan_id=?1",[&plan.id]).unwrap();
    assert!(matches!(
        database.heartbeat_projection(&plan.id, 10_000, None),
        Err(PlanError::Io(_))
    ));
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.interruption.handoff_required',json('false')) WHERE plan_id=?1",[&plan.id]).unwrap();
    database.connection.lock().execute("UPDATE plans SET header_json=json_set(header_json,'$.control_loop.fallback_reason','unknown_failure') WHERE id=?1",[&plan.id]).unwrap();
    assert!(matches!(
        database.heartbeat_projection(&plan.id, 10_000, None),
        Err(PlanError::Quarantined { .. })
    ));
    assert!(matches!(
        database.heartbeat_projection("missing", 10_000, None),
        Err(PlanError::NotFound)
    ));
}
