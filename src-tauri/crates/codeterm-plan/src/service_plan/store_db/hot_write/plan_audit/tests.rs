use super::*;
use crate::plan::{AgentRef, OrchestratorRef, Plan, TaskStatus};
use crate::service_plan::store_db::hot_write::tests::fixtures;
use crate::service_plan::{load_audit, PlanStore};
use std::sync::{Arc, Barrier};

#[test]
fn plan_audit_rows_and_bytes_ignore_unrelated_history_and_body_growth() {
    let mut prior = None;
    for (strides, attempts, body_bytes) in [(0, 0, 0), (2048, 0, 0), (0, 2048, 0), (0, 0, 32768)] {
        let root = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(root.path(), strides, attempts);
        plan.tasks[0].group = Some("group".into());
        plan.tasks[1].body.push_str(&"u".repeat(body_bytes));
        let database = fixtures::database(root.path(), &plan);
        crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&database);
        drop(database);
        let store = PlanStore::open(root.path().join("plans.db")).unwrap();
        fixtures::audit(&store.database);
        load_audit::start(&store);
        store
            .close_group(&plan.id, "group", "closed", AgentRef::from_tab("owner"), 20)
            .unwrap();
        let group = super::super::take_work();
        store
            .add_retrospective(&plan.id, "first", AgentRef::from_tab("owner"), 21)
            .unwrap();
        let first = super::super::take_work();
        let response = store
            .add_retrospective(&plan.id, "second", AgentRef::from_tab("owner"), 22)
            .unwrap();
        let second = super::super::take_work();
        let work = [group, first, second];
        let response_bytes = serde_json::to_vec(&response).unwrap().len();
        println!("fixture={} tasks={} base_content_bytes={} operation=plan_audit strides={strides} unrelated_attempts={attempts} unrelated_body_bytes={body_bytes} changed_rows={:?} work_task_header_attempt_milestone_stride_decoded_serialized_projected_hashed={work:?} response_bytes={response_bytes}", fixtures::SEED, fixtures::TASKS, fixtures::CONTENT_BYTES, fixtures::audit_counts(&store.database));
        assert!(store.plan_loads_for_test().is_empty());
        assert_eq!((group[0], group[1], group[4]), (0, 3, 1));
        assert_eq!((first[0], first[1], first[4]), (0, 2, 1));
        assert_eq!((second[0], second[1], second[4]), (0, 3, 2));
        for entry in work {
            assert_eq!(entry[5], 0);
            assert!(entry[6] < 1024);
        }
        let measured = (work, response_bytes);
        if let Some(prior) = prior {
            assert_eq!(prior, measured);
        }
        prior = Some(measured);
        crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(
            &store.database,
        );
        drop(store);
        let reopened = PlanDatabase::open(root.path().join("plans.db")).unwrap();
        let stored = reopened.load_plan(&plan.id).unwrap().unwrap();
        assert_eq!(stored.tasks, plan.tasks);
        assert_eq!(stored.task_groups[0].key, "group");
        assert_eq!(&stored.strides[..strides], plan.strides.as_slice());
        assert_eq!(stored.strides.len(), strides + 4);
        assert_eq!(
            stored.strides[strides + 2].text,
            crate::service_plan::retrospective::retrospective_supersede_note()
        );
        assert_eq!(stored.strides[strides + 3].id, response);
    }
}

#[test]
fn group_close_failure_rolls_back_declaration_and_stride() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "audit".into(),
        root.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    let mut task = load_audit::task("task", TaskStatus::Pending, None);
    task.group = Some("group".into());
    plan.tasks.push(task);
    drop(fixtures::database(root.path(), &plan));
    let store = PlanStore::open(root.path().join("plans.db")).unwrap();
    store.database.pending_changes.lock().take(&plan.id);
    store.database.connection.lock().execute_batch("CREATE TRIGGER reject_audit_revision BEFORE UPDATE ON plans WHEN json_extract(NEW.header_json,'$._revision') != coalesce(json_extract(OLD.header_json,'$._revision'),0) BEGIN SELECT RAISE(ABORT,'injected audit commit failure'); END;").unwrap();
    assert!(store
        .close_group(&plan.id, "group", "closed", AgentRef::from_tab("owner"), 20)
        .unwrap_err()
        .to_string()
        .contains("injected audit commit failure"));
    assert!(store.take_committed_change(&plan.id).is_none());
    crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&store.database);
    drop(store);
    let reopened = PlanDatabase::open(root.path().join("plans.db")).unwrap();
    assert_eq!(reopened.load_plan(&plan.id).unwrap().unwrap(), plan);
}

#[test]
fn concurrent_retrospectives_append_one_supersede_marker_in_commit_order() {
    let root = tempfile::tempdir().unwrap();
    let plan = Plan::new(
        "audit".into(),
        root.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    drop(fixtures::database(root.path(), &plan));
    let store = Arc::new(PlanStore::open(root.path().join("plans.db")).unwrap());
    let barrier = Arc::new(Barrier::new(3));
    let handles: Vec<_> = ["one", "two"]
        .into_iter()
        .map(|note| {
            let store = store.clone();
            let barrier = barrier.clone();
            let id = plan.id.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store
                    .add_retrospective(&id, note, AgentRef::from_tab("owner"), 20)
                    .unwrap();
            })
        })
        .collect();
    barrier.wait();
    for handle in handles {
        handle.join().unwrap();
    }
    let stored = store.database.load_plan(&plan.id).unwrap().unwrap();
    assert_eq!(stored.strides.len(), 3);
    assert_eq!(
        stored.strides[1].text,
        crate::service_plan::retrospective::retrospective_supersede_note()
    );
    assert_ne!(stored.strides[0].text, stored.strides[2].text);
    assert!(matches!(stored.strides[0].text.as_str(), "one" | "two"));
    assert!(matches!(stored.strides[2].text.as_str(), "one" | "two"));
}
