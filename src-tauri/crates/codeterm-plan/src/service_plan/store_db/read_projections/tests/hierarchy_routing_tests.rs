use super::*;
use crate::plan::{DecisionGate, GateStatus, PlannerRef, TaskStatus};

fn gate(id: &str, status: GateStatus) -> DecisionGate {
    serde_json::from_value(json!({"id":id,"status":status,"question":"opaque question","asked_at":1,"asked_by_tab_id":"planner","routed_to_tab_id":"fallback"})).unwrap()
}

#[test]
fn hierarchy_routing_reads_select_constant_bytes_without_writes() {
    let mut previous = None;
    for (strides, attempts, body) in [(0, 1, 0), (2048, 1, 0), (0, 2048, 0), (0, 1, 32768)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.planner = Some(PlannerRef::Tab {
            tab_id: "planner".into(),
            session_id: Some("session".into()),
            title: Some("Planner".into()),
        });
        plan.tasks[0].status = TaskStatus::InProgress;
        plan.tasks[0].assigned_tab_id = Some("worker".into());
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.gates = (0..12)
            .map(|i| gate(&format!("gate-{i}"), GateStatus::Open))
            .collect();
        plan.gates[11].question.push_str(&"x".repeat(body));
        for g in &mut plan.gates {
            g.context.push_str(&"x".repeat(body));
        }
        plan.planner_requests = serde_json::from_value(
            json!([{"request_id":"request","payload":"opaque","state":"queued","created_at":1}]),
        )
        .unwrap();
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let rehydrate = database.planner_rehydrate_projection(&plan.id).unwrap();
        let notice = database
            .task_review_notice_projection(&plan.id, &plan.tasks[0].id)
            .unwrap();
        let answer = database
            .gate_answer_notice_projection(&plan.id, "gate-0")
            .unwrap();
        let clear = database
            .hierarchy_planner_bindings_projection("planner", true)
            .unwrap();
        let bind = database
            .hierarchy_planner_bindings_projection("planner", false)
            .unwrap();
        let targets = database.relink_targets_projection(&plan.id, true).unwrap();
        let request = database
            .planner_gate_context(&plan.id, Some("request"))
            .unwrap()
            .unwrap();
        let binding = database
            .assigned_task_binding_projection("worker", false)
            .unwrap()
            .unwrap();
        let epoch = database
            .task_owner_epoch_projection(&binding.task_id)
            .unwrap();
        let planner = database.hierarchy_planner_projection(&plan.id).unwrap();
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        assert_eq!(rehydrate.open_count, 12);
        assert_eq!(rehydrate.gates.len(), 10);
        assert_eq!(notice.task_name, plan.tasks[0].name);
        assert_eq!(answer.asker.as_deref(), Some("planner"));
        assert_eq!(clear, vec![(plan.id.clone(), Some("Planner".into()))]);
        assert_eq!(bind, vec![(plan.id.clone(), None)]);
        assert_eq!(targets, vec!["worker", "planner"]);
        assert_eq!(request.request.unwrap().request_id, "request");
        assert_eq!(epoch.owner_epoch, plan.tasks[0].interruption.owner_epoch);
        assert_eq!(planner, plan.planner);
        assert_eq!(metrics.queries, 10);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        let response = serde_json::to_vec(&(
            rehydrate, notice, answer, clear, bind, targets, epoch, planner,
        ))
        .unwrap()
        .len();
        println!("fixture={} consumers=planner_replay,planner_rehydrate,deliver_to_routable_planner,acknowledge_planner_request,notify_owner_task_in_review,nudge_gate_asker,clear_planner_bindings,bind_planner_session,relink_plan_members,relink_plan_hierarchy,persist_local_task_report,observed_handoff_authority,bind_planner_spawn tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(previous) = &previous {
            assert_eq!(&measured, previous);
        }
        previous = Some(measured);
    }
}

#[test]
fn hierarchy_routing_preserves_gate_fallback_relink_trim_order_and_plan_binding_rules() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Routing".into(),
        directory.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.planner = Some(PlannerRef::Tab {
        tab_id: " planner ".into(),
        session_id: None,
        title: Some("kept".into()),
    });
    plan.tasks = vec![
        crate::service_plan::load_audit::task(
            "task_a",
            TaskStatus::InProgress,
            Some("\u{2003}worker\u{2003}"),
        ),
        crate::service_plan::load_audit::task("task_b", TaskStatus::InProgress, Some("worker")),
        crate::service_plan::load_audit::task("task_c", TaskStatus::Paused, Some("paused")),
    ];
    let mut selected = gate("selected", GateStatus::Escalated);
    selected.asked_by_tab_id = None;
    plan.gates = vec![gate("answered", GateStatus::Answered), selected];
    let database = fixtures::database(directory.path(), &plan);
    assert_eq!(
        database.relink_targets_projection(&plan.id, true).unwrap(),
        vec!["worker", "planner"]
    );
    assert_eq!(
        database.relink_targets_projection(&plan.id, false).unwrap(),
        vec!["worker"]
    );
    assert_eq!(
        database
            .planner_rehydrate_projection(&plan.id)
            .unwrap()
            .gates[0]
            .id,
        "selected"
    );
    assert_eq!(
        database
            .gate_answer_notice_projection(&plan.id, "selected")
            .unwrap()
            .asker
            .as_deref(),
        Some("owner")
    );
    assert_eq!(
        database
            .gate_answer_notice_projection(&plan.id, "missing")
            .unwrap()
            .asker
            .as_deref(),
        Some("owner")
    );
    assert!(database
        .hierarchy_planner_bindings_projection(" planner ", true)
        .unwrap()
        .is_empty());
    assert_eq!(
        database
            .hierarchy_planner_bindings_projection(" planner ", false)
            .unwrap(),
        vec![(plan.id.clone(), None)]
    );
    assert_eq!(
        database
            .task_review_notice_projection(&plan.id, "unknown")
            .unwrap()
            .task_name,
        "unknown"
    );
    assert!(matches!(
        database.relink_targets_projection("missing", true),
        Err(PlanError::NotFound)
    ));
    database
        .connection
        .lock()
        .execute_batch("DROP TABLE plans")
        .unwrap();
    assert!(database.planner_rehydrate_projection(&plan.id).is_err());
    assert!(database
        .task_review_notice_projection(&plan.id, "task_a")
        .is_err());
    assert!(database
        .gate_answer_notice_projection(&plan.id, "selected")
        .is_err());
    assert!(database
        .hierarchy_planner_bindings_projection("planner", false)
        .is_err());
    assert!(database.relink_targets_projection(&plan.id, true).is_err());
}
