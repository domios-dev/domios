use super::*;
use crate::service_plan::mutations::agent_lifecycle::AgentLifecyclePlanChange;

fn exit_change() -> AgentLifecyclePlanChange {
    AgentLifecyclePlanChange {
        tab_id: "worker".into(),
        preserve_owned_plans: true,
        retain_owned_plan_for_team_recovery: None,
        preserve_task_bindings_for_owner: None,
        planner_binding: None,
        plan_rebind: None,
    }
}

fn owned_fixture(root: &std::path::Path, strides: usize, attempts: usize) -> Plan {
    let mut plan = fixture(root, strides, attempts);
    plan.team_id = Some("team".into());
    plan.tasks[0].status = TaskStatus::InProgress;
    plan.tasks[0].assigned_tab_id = Some("worker".into());
    plan.tasks[0].lane.tab_id = Some("worker".into());
    plan
}

#[test]
fn agent_and_team_lifecycle_transactions_are_bounded_in_both_directions() {
    let mut baseline = None;
    for (strides, attempts) in [(0, 0), (2048, 0), (0, 2048)] {
        let root = tempfile::tempdir().unwrap();
        let plan = owned_fixture(root.path(), strides, attempts);
        drop(database(root.path(), &plan));
        let store = PlanStore::open(root.path().join("plans.db")).unwrap();
        load_audit::start(&store);
        let transaction = store
            .apply_agent_lifecycle_change(exit_change(), 30)
            .unwrap();
        let mut work = vec![take_work()];
        store.rollback_agent_lifecycle_change(transaction).unwrap();
        work.push(take_work());
        let transaction = store
            .close_team_task_attempts_transaction("team", &["worker".into()], 31)
            .unwrap();
        work.push(take_work());
        store.rollback_team_plan_archive(transaction).unwrap();
        work.push(take_work());
        assert!(store.plan_loads_for_test().is_empty());
        for receipt in &work {
            assert!(
                receipt[0] <= 2
                    && receipt[1] <= 12
                    && receipt[2] <= 1
                    && receipt[3] <= 1
                    && receipt[4] <= 1
            );
            assert!(receipt[5] < 128 * 1024 && receipt[6] < 128 * 1024);
        }
        println!("fixture={SEED} strides={strides} unrelated_attempts={attempts} lifecycle_receipts={work:?}");
        if let Some(baseline) = &baseline {
            assert_eq!(baseline, &work);
        }
        baseline = Some(work);
        let stored = store.database.load_plan(&plan.id).unwrap().unwrap();
        assert_eq!(stored.team_id, plan.team_id);
        assert_eq!(stored.owner, plan.owner);
        assert_eq!(stored.status, plan.status);
        assert_eq!(stored.tasks, plan.tasks);
        assert_eq!(stored.strides, plan.strides);
        assert_eq!(stored.owner_history, plan.owner_history);
        assert_eq!(stored.lifecycle_events, plan.lifecycle_events);
    }
}

#[test]
fn concurrent_prepared_lifecycle_candidates_accept_once() {
    let root = tempfile::tempdir().unwrap();
    let plan = owned_fixture(root.path(), 0, 0);
    drop(database(root.path(), &plan));
    let barrier = Arc::new(Barrier::new(2));
    let writers: Vec<_> = (0..2)
        .map(|_| {
            let store = PlanStore::open(root.path().join("plans.db")).unwrap();
            let prepared = store
                .prepare_agent_lifecycle_changes(vec![exit_change()], 30)
                .unwrap();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.commit_prepared_agent_lifecycle_changes(prepared)
            })
        })
        .collect();
    let results: Vec<_> = writers
        .into_iter()
        .map(|writer| writer.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert!(results.iter().any(|result| result
        .as_ref()
        .err()
        .is_some_and(|error| error.to_string().contains("lifecycle conflict"))));
}

#[test]
fn lifecycle_rollback_refuses_newer_task_state_without_overwriting_it() {
    let root = tempfile::tempdir().unwrap();
    let plan = owned_fixture(root.path(), 0, 0);
    drop(database(root.path(), &plan));
    let store = PlanStore::open(root.path().join("plans.db")).unwrap();
    let transaction = store
        .apply_agent_lifecycle_change(exit_change(), 30)
        .unwrap();
    store
        .update_task_speed(&plan.id, "task_0000", Some("newer"), 31)
        .unwrap();
    assert!(store
        .rollback_agent_lifecycle_change(transaction)
        .unwrap_err()
        .to_string()
        .contains("lifecycle conflict"));
    let task = store
        .database
        .read_task_core(&plan.id, "task_0000")
        .unwrap();
    assert_eq!(task.speed.as_deref(), Some("newer"));
    assert!(task.assigned_tab_id.is_none());
}
