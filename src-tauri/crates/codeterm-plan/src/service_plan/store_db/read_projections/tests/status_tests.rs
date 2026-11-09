use super::*;
use crate::plan::{Milestone, PlanMode, PlanningState, TaskStatus};
use crate::service_plan::state::{build_plan_state_snapshot, PlanStateSnapshot};

fn milestone(id: &str, kind: &str, text: &str, ts: u64) -> Milestone {
    serde_json::from_value(
        json!({"id":id,"kind":kind,"text":text,"ts":ts,"from":{"tab_id":"worker"}}),
    )
    .unwrap()
}

fn snapshot(
    plan: &impl crate::service_plan::state::input::StateSource,
    now: u64,
) -> PlanStateSnapshot {
    let source = plan;
    let input = crate::service_plan::state::input::StateSource::state_input(plan);
    let plan = input.as_ref();
    let mut config = crate::plan::reconcile::ReconcileConfig::new(now);
    config.current_orchestrator_tab_id = plan.owner.tab_id.clone();
    let drift = crate::plan::reconcile::detect_plan_drift(plan, &config, |_| Default::default());
    let cleanup = crate::service_plan::cleanup_debt::detect_plan_cleanup_debt(plan, |_| false);
    let lanes =
        crate::service_plan::state::lane_rows::build_lane_rows(plan, |_| Default::default(), now);
    build_plan_state_snapshot(
        source,
        crate::plan::coverage::compute_coverage_report(plan),
        drift,
        cleanup,
        lanes,
        &Default::default(),
        None,
        now,
    )
}

#[test]
fn sql_status_matches_complete_state_actions_reconcile_counts_and_event_folding() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Status".into(),
        directory.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.mode = PlanMode::Execution;
    plan.planning_state = PlanningState::Approved;
    plan.sections = sections();
    plan.sections[0].body = "APPROVAL and PLAYWRIGHT".into();
    plan.task_groups = serde_json::from_value(json!([{"key":"g","label":"Group"}])).unwrap();
    plan.planning_log = serde_json::from_value(json!([
        {"from":"drafting","to":"structure_ready","by":{"kind":"user"},"at":1},
        {"from":"structure_ready","to":"approved","by":{"kind":"planner","tab_id":"planner"},"at":2}
    ]))
    .unwrap();
    plan.planner_requests=serde_json::from_value(json!([{"request_id":"request","payload":"unrelated prose".repeat(1000),"state":"queued","created_at":1}])).unwrap();
    plan.planner_inbox = serde_json::from_value(
        json!([{"request_id":"request","phase":"delivery_uncertain","updated_at":1}]),
    )
    .unwrap();
    let mut done = crate::service_plan::load_audit::task("task_done", TaskStatus::Done, None);
    done.group = Some("g".into());
    done.section_ids = vec!["sec_a".into()];
    done.status_changed_at = 20;
    done.milestones = vec![
        milestone("r1", "worker_report", "first", 5),
        milestone("r2", "worker_report", "second", 6),
        milestone("todo1", "tab_todo_linked", "\u{2003}Linked\nold line", 8),
    ];
    let mut closed = crate::service_plan::load_audit::attempt("old", "worker");
    closed.executor_title = Some("Old title".into());
    closed.closed_at = Some(21);
    done.execution_attempts = vec![closed];
    let mut review =
        crate::service_plan::load_audit::task("task_review", TaskStatus::InReview, Some("worker"));
    review.status_changed_at = 10;
    review.status_label = Some("review held".into());
    review.review_evidence = serde_json::from_value(
        json!([{"id":"hold","from":{},"verdict":"hold","note":"owner decision","at":11}]),
    )
    .unwrap();
    review.milestones = vec![
        milestone("todo2", "tab_todo_linked", "Linked\nnew line", 9),
        milestone("verdict", "review", "held", 12),
    ];
    let mut open = crate::service_plan::load_audit::attempt("open", "worker");
    open.executor_title = Some("Current title".into());
    review.execution_attempts = vec![open];
    let mut pending =
        crate::service_plan::load_audit::task("task_pending", TaskStatus::Pending, None);
    pending.depends_on = vec!["task_done".into(), "task_missing".into()];
    plan.tasks = vec![done, review, pending];
    plan.gates=serde_json::from_value(json!([{"id":"gate","question":" \u{2003}Owner?\r\nmore","asked_at":10,"status":"open","blocking_task_ids":["task_pending"]}])).unwrap();
    plan.strides = vec![
        milestone("todo3", "tab_todo_linked", "Linked", 10),
        milestone("note", "note", "text\nsecond line", 30),
    ];
    let mut regression = milestone("regression", "regression", "old evidence", 21);
    regression.links = vec!["group:g".into()];
    plan.strides.push(regression);
    let database = fixtures::database(directory.path(), &plan);
    let read = database.status_projection(&plan.id, 100_000, &[]).unwrap();
    let mut projected = snapshot(read.input(), 100_000);
    read.apply_history_facts(&mut projected, 100_000);
    let mut expected = snapshot(&plan, 100_000);
    expected.revision = "1".into();
    assert_eq!(
        serde_json::to_value(&projected).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
    assert_eq!(projected.mission_control.held.verdicts_on_held, 1);
    assert!(
        projected.planning_readiness.has_approval_gate_hint
            && projected.planning_readiness.has_verification_hint
    );
    let linked = projected
        .mission_control
        .event_log
        .entries
        .iter()
        .find(|event| event.kind == "tab_todo_linked")
        .unwrap();
    assert_eq!(linked.count, 3);
    assert_eq!(linked.linked_tasks.len(), 2);
    assert_eq!(projected.mission_control.bounces.total_attempts, 2);
    assert_eq!(projected.undelivered_request_count, 1);
    assert_eq!(
        projected.planner_recovery_action.as_deref(),
        Some("resolve_delivery_uncertain")
    );
}

#[test]
fn sql_status_work_and_materialization_ignore_unrelated_bodies_and_old_history() {
    let mut prior = None;
    for (strides, attempts, extra_body, extra_header) in [
        (0, 1, 0, 0),
        (2048, 1, 0, 0),
        (0, 2048, 0, 0),
        (0, 1, 32768, 0),
        (0, 1, 0, 32768),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.mode = PlanMode::Execution;
        plan.tasks[1].body.push_str(&"x".repeat(extra_body));
        plan.elements = serde_json::from_value(
            json!([{"id":"irrelevant","kind":"note","content":{"text":"x".repeat(extra_header)}}]),
        )
        .unwrap();
        plan.strides.extend((0..256).map(|index| {
            milestone(
                &format!("anchor-{index:04}"),
                "note",
                "fixed recent text",
                1000 + index,
            )
        }));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let read = database.status_projection(&plan.id, 10_000, &[]).unwrap();
        let mut projected = snapshot(read.input(), 10_000);
        read.apply_history_facts(&mut projected, 10_000);
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        let mut response = projected.clone();
        response.window_details(crate::service_plan::state::window::StateWindowRequest::default());
        let response_bytes = serde_json::to_vec(&response).unwrap().len();
        assert_eq!(response.open_lanes.len(), 50);
        assert_eq!(
            response.details.as_ref().unwrap().totals["open_lanes"],
            fixtures::TASKS
        );
        assert!(response_bytes < 100 * 1024);
        assert_eq!(projected.mission_control.event_log.entries.len(), 200);
        assert_eq!(projected.recent_strides.len(), 10);
        assert_eq!(
            projected.mission_control.bounces.total_attempts,
            attempts + 1
        );
        assert_eq!(serialized, 0);
        assert!(metrics.materialized_bytes < 4 * 1024 * 1024);
        assert!(fixtures::audit_counts(&database).is_empty());
        let mut expected = snapshot(&plan, 10_000);
        expected.revision = "1".into();
        assert_eq!(
            serde_json::to_value(&projected).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        println!("fixture={} consumer=state tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={extra_body} extra_header={extra_header} anchor_strides=256 queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response_bytes} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}

#[test]
fn sql_status_projection_is_coherent_and_reports_decode_errors() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "A".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.tasks = vec![crate::service_plan::load_audit::task(
        "task_one",
        TaskStatus::Pending,
        None,
    )];
    plan.tasks[0].name = "A".into();
    let database = Arc::new(fixtures::database(directory.path(), &plan));
    let barrier = Arc::new(Barrier::new(2));
    let writer_barrier = barrier.clone();
    let path = database.path();
    let id = plan.id.clone();
    let writer = std::thread::spawn(move || {
        let mut connection = rusqlite::Connection::open(path).unwrap();
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        for index in 0..16 {
            let text = if index % 2 == 0 { "B" } else { "A" };
            let tx = connection.transaction().unwrap();
            tx.execute(
                "UPDATE plans SET header_json=json_set(header_json,'$.title',?2) WHERE id=?1",
                params![id, text],
            )
            .unwrap();
            tx.execute(
                "UPDATE plan_tasks SET row_json=json_set(row_json,'$.name',?2) WHERE plan_id=?1",
                params![id, text],
            )
            .unwrap();
            writer_barrier.wait();
            tx.commit().unwrap();
            writer_barrier.wait();
        }
    });
    for _ in 0..16 {
        barrier.wait();
        let read = database.status_projection(&plan.id, 10_000, &[]).unwrap();
        assert_eq!(read.input().title, read.input().tasks[0].name);
        barrier.wait();
    }
    writer.join().unwrap();
    database
        .connection
        .lock()
        .execute(
            "UPDATE plan_tasks SET row_json=json_remove(row_json,'$.name') WHERE plan_id=?1",
            [&plan.id],
        )
        .unwrap();
    assert!(matches!(
        database.status_projection(&plan.id, 10_000, &[]),
        Err(PlanError::Quarantined { .. })
    ));
    assert!(matches!(
        database.status_projection("absent", 10_000, &[]),
        Err(PlanError::NotFound)
    ));
}
