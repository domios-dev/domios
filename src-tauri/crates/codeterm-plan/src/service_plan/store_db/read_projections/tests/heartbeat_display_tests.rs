use super::*;
use crate::plan::{HeartbeatLearning, PlanMode, TaskStatus};
use crate::service_plan::heartbeat::view::{HeartbeatParts, HeartbeatView};

#[test]
fn display_preserves_unsuppressed_modes_learning_window_and_shared_blocked_decision() {
    let directory = tempfile::tempdir().unwrap();
    let base = super::curator_notice_tests::decisions(directory.path());
    for mode in [
        PlanMode::Planning,
        PlanMode::Approved,
        PlanMode::Execution,
        PlanMode::Paused,
        PlanMode::Complete,
    ] {
        for completed in [false, true] {
            let case = tempfile::tempdir().unwrap();
            let mut plan = base.clone();
            plan.mode = mode.clone();
            if completed {
                for task in &mut plan.tasks {
                    task.status = TaskStatus::Done;
                }
            }
            plan.heartbeat.current_focus = Some("discarded focus".into());
            plan.heartbeat.active_lanes = vec!["discarded lane".into()];
            plan.heartbeat.rules = vec!["discarded rule".into()];
            plan.heartbeat.learnings = (0..55)
                .map(|index| HeartbeatLearning {
                    id: format!("learning-{index}"),
                    text: if index == 0 {
                        "old excluded text".into()
                    } else {
                        String::new()
                    },
                    ts: index,
                    tag: None,
                })
                .collect();
            plan.heartbeat.learnings.push(HeartbeatLearning {
                id: "owner".into(),
                text: "owner text".into(),
                ts: 60,
                tag: Some("owner".into()),
            });
            let database = fixtures::database(case.path(), &plan);
            let read = database
                .heartbeat_show_projection(&plan.id, 10_000)
                .unwrap();
            let mut heartbeat = plan.heartbeat.clone();
            heartbeat.current_focus = None;
            heartbeat.active_lanes.clear();
            heartbeat.rules.clear();
            heartbeat.learnings.drain(..5);
            assert_eq!(read.heartbeat, heartbeat);
            assert_eq!(
                read.reinjection,
                HeartbeatView::from_plan_at(&plan, 10_000).render_block(&heartbeat, &[]),
                "mode={mode:?} completed={completed}"
            );
            assert_eq!(
                read.blocked_or_gated_count,
                crate::service_plan::state::outstanding_blocked_count(&plan)
            );
            assert!(!read.reinjection.contains("old excluded text"));
            assert!(read.reinjection.contains("owner text"));
        }
    }
}

#[test]
fn display_materialization_excludes_unrelated_history_and_selected_task_body() {
    let mut previous = None;
    for (strides, attempts, body) in [(0, 1, 0), (2048, 1, 0), (0, 2048, 0), (0, 1, 32768)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.tasks[0].body.push_str(&"x".repeat(body));
        let database = fixtures::database(directory.path(), &plan);
        let store = PlanStore::open(database.path()).unwrap();
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let read = store.show_heartbeat(Some(&plan.id), None, None).unwrap();
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&read).unwrap().len();
        assert!(metrics.materialized_bytes < response + 8192);
        assert!(decoded < 8192);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        assert!(store.plan_loads_for_test().is_empty());
        println!("fixture={} consumer=heartbeat_display tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} selected_extra_body={body} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(previous) = &previous {
            assert_eq!(&measured, previous);
        }
        previous = Some(measured);
    }
}

#[test]
fn standalone_heartbeat_parts_preserve_the_held_model_without_constructing_a_plan() {
    let mut reference = Plan::new(String::new(), String::new(), Default::default());
    reference.id = "plan".into();
    reference.loops = serde_json::from_value(json!([
        {"id":"near","target":"work","status":"running","count":1,"budget":2},
        {"id":"closed","target":"work","status":"stopped","count":2,"budget":2}
    ]))
    .unwrap();
    reference.heartbeat.learnings = vec![HeartbeatLearning {
        id: "learning".into(),
        text: "opaque text".into(),
        ts: 1,
        tag: None,
    }];
    let actual = HeartbeatParts::from_snapshot(&reference.id, &reference.loops).render_block(
        &crate::service_plan::heartbeat::view::learning_lines(&reference.heartbeat),
        &[],
    );
    assert_eq!(
        actual,
        HeartbeatView::from_plan_at(&reference, 10_000).render_block(&reference.heartbeat, &[])
    );
}
