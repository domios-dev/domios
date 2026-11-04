use super::*;
use crate::plan::{AgentRef, Plan};
use crate::service_plan::store_db::hot_write::tests::fixtures;
use crate::service_plan::{load_audit, search_index, PlanIncident, PlanStore};
use std::sync::Arc;

#[test]
fn plan_state_rows_and_bytes_ignore_unrelated_history_and_body_growth() {
    let mut prior = None;
    for (strides, attempts, body_bytes) in [(0, 0, 0), (2048, 0, 0), (0, 2048, 0), (0, 0, 32768)] {
        let root = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(root.path(), strides, attempts);
        plan.tasks[1].body.push_str(&"u".repeat(body_bytes));
        let database = fixtures::database(root.path(), &plan);
        crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&database);
        drop(database);
        let store = PlanStore::open(root.path().join("plans.db")).unwrap();
        fixtures::audit(&store.database);
        load_audit::start(&store);
        store
            .update_plan(&plan.id, Some("renamed"), None, Some("execution"), 20)
            .unwrap();
        let update = super::super::take_work();
        store
            .archive_with_incident(
                &plan.id,
                Some(PlanIncident {
                    kind: "decision".into(),
                    text: "archive incident".into(),
                    from: AgentRef::from_tab("owner"),
                }),
                21,
            )
            .unwrap();
        let archive = super::super::take_work();
        store.unarchive(&plan.id, 22).unwrap();
        let restore = super::super::take_work();
        let work = [update, archive, restore];
        println!("fixture={} tasks={} base_content_bytes={} operation=plan_state strides={strides} unrelated_attempts={attempts} unrelated_body_bytes={body_bytes} changed_rows={:?} work_task_header_attempt_milestone_stride_decoded_serialized_projected_hashed={work:?} response_bytes=4", fixtures::SEED, fixtures::TASKS, fixtures::CONTENT_BYTES, fixtures::audit_counts(&store.database));
        assert!(store.plan_loads_for_test().is_empty());
        assert_eq!((update[0], update[1], update[4]), (0, 2, 0));
        assert_eq!((archive[0], archive[1], archive[4]), (0, 4, 1));
        assert_eq!((restore[0], restore[1], restore[4]), (0, 3, 0));
        for entry in work {
            assert!(entry[5] < 1024 && entry[6] < 1024);
        }
        if let Some(prior) = prior {
            assert_eq!(prior, work);
        }
        prior = Some(work);
        assert_eq!(store.index.read()[&plan.id].title, "renamed");
        assert_eq!(store.index.read()[&plan.id].status, PlanStatus::Active);
        crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(
            &store.database,
        );
        drop(store);
        let reopened = PlanDatabase::open(root.path().join("plans.db")).unwrap();
        let stored = reopened.load_plan(&plan.id).unwrap().unwrap();
        assert_eq!(stored.tasks, plan.tasks);
        assert_eq!(&stored.strides[..strides], plan.strides.as_slice());
        assert_eq!(stored.strides.len(), strides + 1);
        assert_eq!(stored.strides[strides].text, "archive incident");
        assert_eq!(stored.mode, PlanMode::Execution);
        let baseline_root = tempfile::tempdir().unwrap();
        let baseline = fixtures::database(baseline_root.path(), &plan);
        crate::service_plan::store_sync::take_decoded_row_bytes();
        store_rows::take_serialized_row_bytes();
        let mut aggregate = baseline.load_plan(&plan.id).unwrap().unwrap();
        aggregate.title = stored.title.clone();
        aggregate.mode = stored.mode.clone();
        aggregate.strides = stored.strides.clone();
        aggregate.lifecycle_events = stored.lifecycle_events.clone();
        aggregate.updated_at = 22;
        let writes = baseline.persist_plan_rows(&aggregate, None).unwrap();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = store_rows::take_serialized_row_bytes();
        println!("fixture={} operation=plan_state_roundtrip baseline=load_plan+persist_plan_rows strides={strides} unrelated_attempts={attempts} unrelated_body_bytes={body_bytes} decoded_bytes={decoded} serialized_bytes={serialized} writes={writes:?}", fixtures::SEED);
        assert!(decoded > 31 * 1024 * 1024 && serialized > 31 * 1024 * 1024);
        assert_eq!(
            stored
                .lifecycle_events
                .iter()
                .map(|event| event.kind.as_str())
                .collect::<Vec<_>>(),
            vec!["archived", "activated"]
        );
    }
}

#[test]
fn failed_archive_rolls_back_status_lifecycle_incident_revision_and_light_index() {
    let root = tempfile::tempdir().unwrap();
    let plan = Plan::new(
        "state".into(),
        root.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    drop(fixtures::database(root.path(), &plan));
    let store = PlanStore::open(root.path().join("plans.db")).unwrap();
    let index = store.index.read()[&plan.id].clone();
    store.database.pending_changes.lock().take(&plan.id);
    store.database.connection.lock().execute_batch("CREATE TRIGGER reject_state_revision BEFORE UPDATE ON plans WHEN json_extract(NEW.header_json,'$._revision') != coalesce(json_extract(OLD.header_json,'$._revision'),0) BEGIN SELECT RAISE(ABORT,'injected state commit failure'); END;").unwrap();
    assert!(store
        .archive_with_incident(
            &plan.id,
            Some(PlanIncident {
                kind: "decision".into(),
                text: "rejected".into(),
                from: AgentRef::from_tab("owner")
            }),
            20
        )
        .unwrap_err()
        .to_string()
        .contains("injected state commit failure"));
    assert_eq!(store.index.read()[&plan.id], index);
    assert!(store.take_committed_change(&plan.id).is_none());
    crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&store.database);
    drop(store);
    let reopened = PlanDatabase::open(root.path().join("plans.db")).unwrap();
    assert_eq!(reopened.load_plan(&plan.id).unwrap().unwrap(), plan);
}

#[test]
fn plan_activation_preserves_explicit_unarchive_and_team_exclusivity() {
    let root = tempfile::tempdir().unwrap();
    let mut archived = Plan::new(
        "archived".into(),
        root.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    archived.team_id = Some("team".into());
    archived.status = PlanStatus::Archived;
    let database = fixtures::database(root.path(), &archived);
    let mut active = Plan::new(
        "active".into(),
        root.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    active.team_id = Some("team".into());
    database.persist_plan_rows(&active, None).unwrap();
    crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&database);
    drop(database);
    let store = PlanStore::open(root.path().join("plans.db")).unwrap();
    assert!(matches!(
        store.update_plan(&archived.id, None, Some(PlanStatus::Active), None, 20),
        Err(PlanError::PlanActivationRequiresUnarchive { .. })
    ));
    assert!(matches!(
        store.unarchive(&archived.id, 21),
        Err(PlanError::TeamAlreadyHasActivePlan { .. })
    ));
    assert_eq!(
        store.database.load_plan(&archived.id).unwrap().unwrap(),
        archived
    );
    assert_eq!(
        store.database.load_plan(&active.id).unwrap().unwrap(),
        active
    );
}

#[test]
fn archive_publishes_search_status_after_commit_without_loading_plan() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(root.path(), 0, 0);
    plan.tasks.truncate(2);
    for task in &mut plan.tasks {
        task.body.clear();
    }
    drop(fixtures::database(root.path(), &plan));
    search_index::init_search_port(Arc::new(crate::service_plan::test_support::TestSearchPort));
    let store = PlanStore::open(root.path().join("plans.db")).unwrap();
    store.mirror_plan_index(&plan).unwrap();
    search_index::flush_index_queue_for_test(root.path());
    load_audit::start(&store);
    store
        .update_plan(&plan.id, Some("renamed"), None, None, 19)
        .unwrap();
    search_index::flush_index_queue_for_test(root.path());
    store.archive(&plan.id, 20).unwrap();
    assert_eq!(
        store
            .database
            .load_index_row(&plan.id)
            .unwrap()
            .unwrap()
            .status,
        PlanStatus::Archived
    );
    search_index::flush_index_queue_for_test(root.path());
    assert!(store.plan_loads_for_test().is_empty());
    let index = rusqlite::Connection::open(search_index::project_index_path(root.path())).unwrap();
    let task_title: String = index
        .query_row(
            "SELECT title FROM mem_items WHERE id=?1",
            [format!("planunit_{}_task_task_0000", plan.id)],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(task_title, "renamed task task_0000");
    let statuses: Vec<String> = index.prepare("SELECT json_extract(source,'$.status') FROM mem_items WHERE json_extract(source,'$.plan_id')=?1 ORDER BY id").unwrap().query_map([&plan.id], |row| row.get(0)).unwrap().map(Result::unwrap).collect();
    assert_eq!(statuses.len(), 3);
    assert!(statuses
        .iter()
        .all(|status| status == "archived" || status.starts_with("archived:")));
}
