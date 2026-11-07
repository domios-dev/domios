use super::*;
use crate::plan::{CuratorRound, PlanMode, PlannerRef, TaskStatus};
use crate::service_plan::{KpiPlanContextRead, TaskBindingRead};

#[test]
fn binding_context_preserves_priority_status_mode_planner_and_ambiguity() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Bindings".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.id = "pl_a".into();
    plan.updated_at = 10;
    plan.mode = PlanMode::Execution;
    plan.owner.tab_id = Some("worker".into());
    plan.planner = Some(PlannerRef::Tab {
        tab_id: "worker".into(),
        session_id: None,
        title: None,
    });
    plan.tasks = vec![
        crate::service_plan::load_audit::task("paused", TaskStatus::Paused, Some("worker")),
        crate::service_plan::load_audit::task("pending", TaskStatus::Pending, Some("worker")),
    ];
    let database = fixtures::database(directory.path(), &plan);
    let mut newer = plan.clone();
    newer.id = "pl_new".into();
    newer.updated_at = 11;
    newer.mode = PlanMode::Approved;
    database.persist_plan(&newer, None).unwrap();
    let binding = |id: &str, task: &str| {
        Some(TaskBindingRead {
            plan_id: id.into(),
            task_id: task.into(),
        })
    };
    assert_eq!(
        database
            .assigned_task_binding_projection("worker", false)
            .unwrap(),
        binding("pl_new", "paused")
    );
    assert_eq!(
        database
            .assigned_task_binding_projection("worker", true)
            .unwrap(),
        binding("pl_a", "pending")
    );
    assert_eq!(
        database.planner_binding_projection("worker").unwrap(),
        Some("pl_new".into())
    );
    assert_eq!(
        database
            .taskless_plan_bindings_projection("worker")
            .unwrap(),
        vec!["pl_a", "pl_new"]
    );
    assert_eq!(
        database.kpi_plan_context_projection("worker").unwrap(),
        KpiPlanContextRead {
            plan_id: Some("pl_new".into()),
            task_id: Some("paused".into())
        }
    );
    database
        .connection
        .lock()
        .execute(
            "UPDATE plan_tasks SET row_json=json_set(row_json,'$.assigned_tab_id','other')",
            [],
        )
        .unwrap();
    assert_eq!(
        database.kpi_plan_context_projection("worker").unwrap(),
        KpiPlanContextRead::default()
    );
    database.connection.lock().execute("UPDATE plans SET header_json=json_set(header_json,'$.planner',json('{\"kind\":\"waived\",\"note\":\"solo\",\"tab_id\":\"worker\"}')) WHERE id='pl_new'",[]).unwrap();
    assert_eq!(
        database.planner_binding_projection("worker").unwrap(),
        Some("pl_a".into())
    );
    assert_eq!(
        database
            .taskless_plan_bindings_projection("worker")
            .unwrap(),
        vec!["pl_a"]
    );
    assert_eq!(
        database.kpi_plan_context_projection("worker").unwrap(),
        KpiPlanContextRead {
            plan_id: Some("pl_a".into()),
            task_id: None
        }
    );
    assert!(database
        .assigned_task_binding_projection("missing", false)
        .unwrap()
        .is_none());
    assert!(database
        .planner_binding_projection("missing")
        .unwrap()
        .is_none());
    take_metrics();
    assert!(database
        .assigned_task_binding_projection(" ", false)
        .unwrap()
        .is_none());
    assert!(database.planner_binding_projection(" ").unwrap().is_none());
    assert!(database
        .taskless_plan_bindings_projection(" ")
        .unwrap()
        .is_empty());
    assert_eq!(
        database.kpi_plan_context_projection(" ").unwrap(),
        KpiPlanContextRead::default()
    );
    assert_eq!(take_metrics().queries, 0);
}

#[test]
fn binding_context_has_fixed_materialization_and_never_masks_decode_errors() {
    let mut prior = None;
    for (strides, attempts, extra_body, rounds) in [
        (0, 1, 0, 0),
        (2048, 1, 0, 0),
        (0, 2048, 0, 0),
        (0, 1, 32768, 0),
        (0, 1, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.mode = PlanMode::Execution;
        plan.tasks[0].assigned_tab_id = Some("worker".into());
        plan.tasks[1].body.push_str(&"x".repeat(extra_body));
        plan.planner = Some(PlannerRef::Tab {
            tab_id: "planner".into(),
            session_id: None,
            title: None,
        });
        plan.strides.extend(fixtures::round_strides(
            (0..rounds)
                .map(|number| CuratorRound {
                    number,
                    ..Default::default()
                })
                .collect(),
        ));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let assigned = database
            .assigned_task_binding_projection("worker", false)
            .unwrap();
        let pending = database
            .assigned_task_binding_projection("worker", true)
            .unwrap();
        let planner = database.planner_binding_projection("planner").unwrap();
        let taskless = database
            .taskless_plan_bindings_projection("planner")
            .unwrap();
        let kpi = database.kpi_plan_context_projection("worker").unwrap();
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        let response_bytes = serde_json::to_vec(&(assigned, pending, planner, taskless, kpi))
            .unwrap()
            .len();
        assert_eq!(metrics.queries, 5);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=binding_context tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={extra_body} rounds={rounds} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response_bytes} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response_bytes);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
        database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.id',17) WHERE plan_id=?1 AND position=0",[&plan.id]).unwrap();
        assert!(database
            .assigned_task_binding_projection("worker", false)
            .is_err());
        assert!(database.kpi_plan_context_projection("worker").is_err());
    }
}

#[test]
fn kpi_assignment_and_taskless_fallback_share_one_committed_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Task".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.id = "pl_task".into();
    plan.mode = PlanMode::Execution;
    plan.tasks = vec![crate::service_plan::load_audit::task(
        "task",
        TaskStatus::InProgress,
        Some("worker"),
    )];
    let database = fixtures::database(directory.path(), &plan);
    let mut owner = plan.clone();
    owner.id = "pl_owner".into();
    owner.tasks.clear();
    owner.owner.tab_id = Some("other".into());
    database.persist_plan(&owner, None).unwrap();
    let path = database.path();
    let barrier = Arc::new(Barrier::new(2));
    let writer_barrier = barrier.clone();
    let writer = std::thread::spawn(move || {
        let mut connection = rusqlite::Connection::open(path).unwrap();
        connection
            .busy_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        for index in 0..16 {
            let (assigned, owner) = if index % 2 == 0 {
                ("other", "worker")
            } else {
                ("worker", "other")
            };
            let tx = connection.transaction().unwrap();
            tx.execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.assigned_tab_id',?1) WHERE plan_id='pl_task'",[assigned]).unwrap();
            tx.execute("UPDATE plans SET header_json=json_set(header_json,'$.owner.tab_id',?1) WHERE id='pl_owner'",[owner]).unwrap();
            writer_barrier.wait();
            tx.commit().unwrap();
            writer_barrier.wait();
        }
    });
    for _ in 0..16 {
        barrier.wait();
        let context = database.kpi_plan_context_projection("worker").unwrap();
        assert!(matches!(
            (context.plan_id.as_deref(), context.task_id.as_deref()),
            (Some("pl_task"), Some("task")) | (Some("pl_owner"), None)
        ));
        barrier.wait();
    }
    writer.join().unwrap();
}
