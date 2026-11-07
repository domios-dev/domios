use super::*;
use crate::plan::TaskStatus;
use crate::service_plan::milestone_gates::{self, RegressionInput};

fn gate_groups(plan: &mut Plan) {
    for task in &mut plan.tasks {
        task.group = None;
    }
    plan.task_groups = serde_json::from_value(json!([
        {"key":"g","label":"First"}, {"key":"g","label":"Ignored"},
        {"key":"cancelled","label":"Cancelled"}, {"key":"open","label":"Open"}
    ]))
    .unwrap();
    for (index, key, status, at) in [
        (0, "g", TaskStatus::Done, u64::MAX - 1),
        (1, "g", TaskStatus::Cancelled, 9),
        (2, "undeclared", TaskStatus::Done, 10),
        (3, "cancelled", TaskStatus::Cancelled, 10),
        (4, "open", TaskStatus::Pending, 10),
    ] {
        plan.tasks[index].group = Some(key.into());
        plan.tasks[index].status = status;
        plan.tasks[index].status_changed_at = at;
    }
    plan.strides.extend(serde_json::from_value::<Vec<crate::plan::Milestone>>(json!([
        {"id":"stale","kind":"regression","text":"evidence","from":{},"ts":u64::MAX-2,"links":["group:g"]},
        {"id":"retro","kind":"group_close","text":"retrospective","from":{},"ts":u64::MAX-2,"links":["group:g"]},
        {"id":"satisfied","kind":"regression","text":"evidence","from":{},"ts":10,"links":["group:undeclared"]}
    ])).unwrap());
    plan.regression_gate_overrides = serde_json::from_value(json!([
        {"actor":{},"reason":"Stale waiver","gates":["g"],"ts":u64::MAX-2}
    ]))
    .unwrap();
}

#[test]
fn regression_sql_keeps_undeclared_groups_first_labels_and_unsigned_staleness() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(directory.path(), 0, 1);
    gate_groups(&mut plan);
    let database = fixtures::database(directory.path(), &plan);
    let store = PlanStore::open(database.path()).unwrap();
    let input = store.regression_gate_projection(&plan.id).unwrap().unwrap();
    assert_eq!(input, RegressionInput::from(&plan));
    assert_eq!(
        input
            .groups
            .iter()
            .map(|group| group.key.as_str())
            .collect::<Vec<_>>(),
        vec!["g", "undeclared"]
    );
    assert_eq!(input.groups[0].label, "First");
    assert_eq!(
        milestone_gates::blocking_regression_gates(input.clone())
            .iter()
            .map(|gate| gate.key.as_str())
            .collect::<Vec<_>>(),
        vec!["g"]
    );
    assert_eq!(
        milestone_gates::regression_dispatch_decision(input),
        milestone_gates::regression_dispatch_decision(&plan)
    );
    for stride in &mut plan.strides {
        if stride.links.contains(&"group:g".to_owned()) {
            stride.ts = u64::MAX;
        }
    }
    plan.regression_gate_overrides[0].ts = u64::MAX;
    database.persist_plan_rows(&plan, None).unwrap();
    let input = store.regression_gate_projection(&plan.id).unwrap().unwrap();
    assert_eq!(input, RegressionInput::from(&plan));
    assert!(milestone_gates::blocking_regression_gates(input.clone()).is_empty());
    assert_eq!(
        milestone_gates::pending_group_retrospectives(input)
            .iter()
            .map(|gate| gate.key.as_str())
            .collect::<Vec<_>>(),
        vec!["undeclared"]
    );
    assert!(store
        .regression_gate_projection("missing")
        .unwrap()
        .is_none());
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.status_changed_at',-1) WHERE plan_id=?1 AND task_id=?2",params![plan.id,plan.tasks[0].id]).unwrap();
    assert!(store.regression_gate_projection(&plan.id).is_err());
    assert!(store.plan_loads_for_test().is_empty());
}

fn cleanup_lanes(plan: &mut Plan, directory: &std::path::Path) {
    plan.canonical_project_path = "canonical project".into();
    plan.tasks[0].status = TaskStatus::Done;
    plan.tasks[0].lane = serde_json::from_value(json!({
        "tab_id":"live","worktree_path":directory.display().to_string(),"branch":"lane/retained",
        "cleanup_status":"deferred","cleanup_hints":["waiting for integration"]
    }))
    .unwrap();
    plan.tasks[1].status = TaskStatus::Cancelled;
    plan.tasks[1].lane =
        serde_json::from_value(json!({"tab_id":"","cleanup_status":"pending"})).unwrap();
    plan.tasks[1].assigned_tab_id = Some("assigned".into());
    plan.tasks[2].status = TaskStatus::Pending;
    plan.tasks[2].lane =
        serde_json::from_value(json!({"tab_id":"unselected","cleanup_hints":["pending lane"]}))
            .unwrap();
}

#[test]
fn cleanup_sql_preserves_lane_precedence_terminal_filter_and_shared_decisions() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(directory.path(), 0, 1);
    cleanup_lanes(&mut plan, directory.path());
    let database = fixtures::database(directory.path(), &plan);
    let store = PlanStore::open(database.path()).unwrap();
    let mut probed = Vec::new();
    let actual = store
        .cleanup_debt_projection(&plan.id, |tab| {
            probed.push(tab.to_owned());
            tab == "live"
        })
        .unwrap()
        .unwrap();
    let expected =
        crate::service_plan::cleanup_debt::detect_plan_cleanup_debt(&plan, |tab| tab == "live");
    assert_eq!(actual, expected);
    assert!(probed.contains(&String::new()));
    assert!(!probed
        .iter()
        .any(|tab| tab == "assigned" || tab == "unselected"));
    assert_eq!(actual[0].task_id, plan.tasks[0].id);
    assert!(store
        .cleanup_debt_projection("missing", |_| panic!("no missing plan probes"))
        .unwrap()
        .is_none());
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.lane.cleanup_hints',42) WHERE plan_id=?1 AND task_id=?2",params![plan.id,plan.tasks[0].id]).unwrap();
    assert!(store.cleanup_debt_projection(&plan.id, |_| false).is_err());
    assert!(store.plan_loads_for_test().is_empty());
}

#[test]
fn regression_and_cleanup_reads_ignore_unrequested_growth_without_writes() {
    let mut prior = None;
    for (strides, attempts, extra, history, rounds) in [
        (0, 1, 0, 0, 0),
        (2048, 1, 0, 0, 0),
        (0, 2048, 0, 0, 0),
        (0, 1, 32768, 0, 0),
        (0, 1, 0, 2048, 0),
        (0, 1, 0, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        gate_groups(&mut plan);
        cleanup_lanes(&mut plan, directory.path());
        plan.tasks[0].body.push_str(&"x".repeat(extra));
        for stride in &mut plan.strides {
            stride.text.push_str(&"x".repeat(extra));
        }
        plan.regression_gate_overrides[0]
            .reason
            .push_str(&"x".repeat(extra));
        for number in 0..history {
            let mut attempt =
                crate::service_plan::load_audit::attempt(&format!("old-{number}"), "other");
            attempt.closed_at = Some(1);
            plan.tasks[0].execution_attempts.push(attempt);
        }
        plan.strides.extend(fixtures::round_strides(
            (0..rounds)
                .map(|number| crate::plan::CuratorRound {
                    number,
                    ended_at: number,
                    ..Default::default()
                })
                .collect(),
        ));
        let expected = RegressionInput::from(&plan);
        let database = fixtures::database(directory.path(), &plan);
        let store = PlanStore::open(database.path()).unwrap();
        fixtures::audit(&database);
        crate::service_plan::store_sync::take_decoded_row_bytes();
        database.load_plan(&plan.id).unwrap().unwrap();
        let baseline = crate::service_plan::store_sync::take_decoded_row_bytes();
        take_metrics();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let input = store.regression_gate_projection(&plan.id).unwrap().unwrap();
        assert_eq!(input, expected);
        let cleanup = store
            .cleanup_debt_projection(&plan.id, |tab| tab == "live")
            .unwrap()
            .unwrap();
        assert_eq!(
            cleanup,
            crate::service_plan::cleanup_debt::detect_plan_cleanup_debt(&plan, |tab| tab == "live")
        );
        let response = serde_json::to_vec(&json!([
            milestone_gates::blocking_regression_gates(input),
            cleanup
        ]))
        .unwrap()
        .len();
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        assert_eq!(metrics.queries, 4);
        assert!(metrics.materialized_bytes < 262144);
        assert!(decoded < 262144);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        assert!(store.plan_loads_for_test().is_empty());
        println!("fixture={} consumer=regression_cleanup tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={extra} unselected_target_history={history} rounds={rounds} baseline_decoded_json_bytes={baseline} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}

#[test]
fn regression_groups_and_evidence_share_a_committed_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(directory.path(), 0, 1);
    gate_groups(&mut plan);
    plan.tasks.truncate(1);
    plan.tasks[0].status_changed_at = 10;
    plan.strides
        .retain(|stride| stride.kind == "regression" && stride.links.contains(&"group:g".into()));
    plan.strides[0].ts = 10;
    let database = fixtures::database(directory.path(), &plan);
    let store = PlanStore::open(database.path()).unwrap();
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
            let at = if index % 2 == 0 { 20 } else { 10 };
            let transaction = connection.transaction().unwrap();
            transaction.execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.status_changed_at',?2) WHERE plan_id=?1",params![id,at]).unwrap();
            transaction.execute("UPDATE plan_strides SET row_json=json_set(row_json,'$.ts',?2) WHERE plan_id=?1",params![id,at]).unwrap();
            writer_barrier.wait();
            transaction.commit().unwrap();
            writer_barrier.wait();
        }
    });
    let mut coherent = true;
    for _ in 0..16 {
        barrier.wait();
        let input = store.regression_gate_projection(&plan.id);
        barrier.wait();
        coherent &= input.is_ok_and(|input| {
            input.is_some_and(|input| {
                input.groups.len() == 1
                    && input.groups[0].regression_at == Some(input.groups[0].completed_at)
            })
        });
    }
    writer.join().unwrap();
    assert!(
        coherent,
        "group completion and evidence must come from the same commit"
    );
}
