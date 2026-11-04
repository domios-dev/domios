use super::*;
use crate::plan::{OrchestratorRef, Plan};
use crate::service_plan::store_db::hot_write::tests::fixtures;
use crate::service_plan::{load_audit, PlanStore};
use std::sync::{Arc, Barrier};

fn section(id: &str, title: &str) -> Section {
    Section {
        id: id.into(),
        title: title.into(),
        handle: Some(id.into()),
        tags: vec![],
        body: "body".into(),
        subsections: vec![],
        canvas: Default::default(),
        attachments: vec![],
    }
}

#[test]
fn section_family_rows_and_bytes_ignore_unrelated_history_and_other_root_bodies() {
    let mut prior = None;
    for (strides, attempts, task_body_bytes, section_body_bytes) in [
        (0, 0, 0, 0),
        (2048, 0, 0, 0),
        (0, 2048, 0, 0),
        (0, 0, 32768, 0),
        (0, 0, 0, 32768),
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(root.path(), strides, attempts);
        plan.sections = vec![
            section("selected", "Selected"),
            section("unrelated", "Unrelated"),
        ];
        plan.tasks[1].body.push_str(&"u".repeat(task_body_bytes));
        plan.sections[1]
            .body
            .push_str(&"s".repeat(section_body_bytes));
        let database = fixtures::database(root.path(), &plan);
        crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&database);
        drop(database);
        let store = PlanStore::open(root.path().join("plans.db")).unwrap();
        fixtures::audit(&store.database);
        load_audit::start(&store);
        let id = store
            .add_section(
                &plan.id,
                "Child",
                "child body",
                Some("selected"),
                &[],
                None,
                Some("child-handle"),
                20,
            )
            .unwrap();
        let add = super::super::take_work();
        store
            .update_section(
                &plan.id,
                "child-handle",
                Some("Renamed"),
                Some("edited"),
                &["area:sql".into()],
                None,
                None,
                21,
            )
            .unwrap();
        let update = super::super::take_work();
        let updated = store.database.selected_section(&plan.id, &id).unwrap();
        assert_eq!(updated.title, "Renamed");
        assert_eq!(updated.body, "edited");
        assert_eq!(updated.tags, vec!["area:sql"]);
        store.rm_section(&plan.id, &id, 22).unwrap();
        let remove = super::super::take_work();
        let work = [add, update, remove];
        let response_bytes = serde_json::to_vec(&id).unwrap().len();
        println!("fixture={} tasks={} base_content_bytes={} operation=section_family strides={strides} unrelated_attempts={attempts} unrelated_task_body_bytes={task_body_bytes} unrelated_root_body_bytes={section_body_bytes} storage_entity=top_level_section_with_subsections changed_rows={:?} work_task_header_attempt_milestone_stride_decoded_serialized_projected_hashed_section={work:?} response_bytes={response_bytes}", fixtures::SEED, fixtures::TASKS, fixtures::CONTENT_BYTES, fixtures::audit_counts(&store.database));
        assert!(store.plan_loads_for_test().is_empty());
        assert_eq!(
            fixtures::audit_counts(&store.database),
            vec![("plan_sections".into(), 3), ("plans".into(), 3)]
        );
        for entry in work {
            assert_eq!(&entry[..5], &[0, 1, 0, 0, 0]);
            assert_eq!(entry[9], 1);
            assert!(entry[5] < 2048 && entry[6] < 2048 && entry[8] < 2048);
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
        assert_eq!(stored.strides, plan.strides);
        assert_eq!(stored.sections, plan.sections);
        let baseline_root = tempfile::tempdir().unwrap();
        let baseline = fixtures::database(baseline_root.path(), &plan);
        fixtures::baseline_operation(&baseline, &plan.id, "section_add", |aggregate| {
            aggregate.sections[0]
                .subsections
                .push(section("baseline-child", "Child"));
        });
    }
}

#[test]
fn section_removal_failure_preserves_subtree_and_committed_revision() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "section".into(),
        root.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    let mut selected = section("selected", "Selected");
    selected.subsections.push(section("child", "Child"));
    plan.sections.push(selected);
    drop(fixtures::database(root.path(), &plan));
    let store = PlanStore::open(root.path().join("plans.db")).unwrap();
    store.database.pending_changes.lock().take(&plan.id);
    store.database.connection.lock().execute_batch("CREATE TRIGGER reject_section_revision BEFORE UPDATE ON plans WHEN json_extract(NEW.header_json,'$._revision') != coalesce(json_extract(OLD.header_json,'$._revision'),0) BEGIN SELECT RAISE(ABORT,'injected section commit failure'); END;").unwrap();
    assert!(store
        .rm_section(&plan.id, "selected", 20)
        .unwrap_err()
        .to_string()
        .contains("injected section commit failure"));
    assert!(store.take_committed_change(&plan.id).is_none());
    crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&store.database);
    drop(store);
    let reopened = PlanDatabase::open(root.path().join("plans.db")).unwrap();
    assert_eq!(reopened.load_plan(&plan.id).unwrap().unwrap(), plan);
}

#[test]
fn concurrent_section_additions_preserve_both_children_and_unique_handles() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "section".into(),
        root.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.sections.push(section("selected", "Selected"));
    drop(fixtures::database(root.path(), &plan));
    let store = Arc::new(PlanStore::open(root.path().join("plans.db")).unwrap());
    let barrier = Arc::new(Barrier::new(3));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let store = store.clone();
            let barrier = barrier.clone();
            let id = plan.id.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store
                    .add_section(&id, "Child", "body", Some("selected"), &[], None, None, 20)
                    .unwrap();
            })
        })
        .collect();
    barrier.wait();
    for handle in handles {
        handle.join().unwrap();
    }
    assert!(store.plan_loads_for_test().is_empty());
    let stored = store.database.load_plan(&plan.id).unwrap().unwrap();
    let children = &stored.sections[0].subsections;
    assert_eq!(children.len(), 2);
    assert_ne!(children[0].id, children[1].id);
    assert_eq!(children[0].handle.as_deref(), Some("child"));
    assert_eq!(children[1].handle.as_deref(), Some("child-2"));
}
