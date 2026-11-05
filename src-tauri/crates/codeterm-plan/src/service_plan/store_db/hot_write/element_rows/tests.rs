use super::*;
use crate::plan::{ElementCanvas, OrchestratorRef, Plan, Section};
use crate::service_plan::store_db::hot_write::tests::fixtures;
use crate::service_plan::{load_audit, PlanStore};
use std::sync::{Arc, Barrier};

#[test]
fn canvas_family_rows_and_bytes_ignore_unrelated_history_and_body_growth() {
    let mut prior = None;
    for (strides, attempts, body_bytes) in [(0, 0, 0), (2048, 0, 0), (0, 2048, 0), (0, 0, 32768)] {
        let root = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(root.path(), strides, attempts);
        plan.tasks[1].body.push_str(&"u".repeat(body_bytes));
        plan.sections.push(Section {
            id: "section".into(),
            title: "Section".into(),
            handle: Some("section".into()),
            tags: vec![],
            body: "body".into(),
            subsections: vec![],
            canvas: Default::default(),
            attachments: vec![],
        });
        plan.tasks[0].milestones.push(serde_json::from_value(serde_json::json!({"id":"milestone","kind":"decision","text":"text","from":{},"ts":1})).unwrap());
        let database = fixtures::database(root.path(), &plan);
        crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&database);
        drop(database);
        let store = PlanStore::open(root.path().join("plans.db")).unwrap();
        fixtures::audit(&store.database);
        load_audit::start(&store);
        let mut work = Vec::new();
        let id = store
            .add_note(&plan.id, "note", vec![], None, None, 20)
            .unwrap();
        work.push(super::super::take_work());
        store
            .update_element(
                &plan.id,
                &id,
                Some("changed"),
                Some(serde_json::json!({"text":"edited"})),
                Some(vec!["link".into()]),
                21,
            )
            .unwrap();
        work.push(super::super::take_work());
        store
            .set_layout(&plan.id, &format!("el:{id}"), Some(1.0), Some(2.0), 22)
            .unwrap();
        work.push(super::super::take_work());
        store.rm_element(&plan.id, &id, 23).unwrap();
        work.push(super::super::take_work());
        for target in ["task:task_0000", "section:section", "milestone:milestone"] {
            store
                .set_layout(&plan.id, target, Some(3.0), Some(4.0), 24)
                .unwrap();
            work.push(super::super::take_work());
        }
        let response_bytes = serde_json::to_vec(&id).unwrap().len();
        println!("fixture={} tasks={} base_content_bytes={} operation=canvas_family strides={strides} unrelated_attempts={attempts} unrelated_body_bytes={body_bytes} changed_rows={:?} work_task_header_attempt_milestone_stride_decoded_serialized_projected_hashed_section_element_heartbeat={work:?} response_bytes={response_bytes}", fixtures::SEED, fixtures::TASKS, fixtures::CONTENT_BYTES, fixtures::audit_counts(&store.database));
        assert!(store.plan_loads_for_test().is_empty());
        for entry in &work[..4] {
            assert_eq!(&entry[..5], &[0, 1, 0, 0, 0]);
            assert_eq!(entry[10], 1);
            assert!(entry[5] < 1024 && entry[6] < 1024);
        }
        assert_eq!(
            (work[4][0], work[5][9], work[6][0], work[6][3]),
            (1, 1, 1, 1)
        );
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
        assert!(stored.elements.is_empty());
        assert_eq!(stored.tasks[0].canvas.x, Some(3.0));
        assert_eq!(stored.tasks[0].updated_at, plan.tasks[0].updated_at);
        assert_eq!(stored.sections[0].canvas.y, Some(4.0));
        assert_eq!(stored.tasks[0].milestones[0].canvas.x, Some(3.0));
        assert_eq!(stored.tasks[1], plan.tasks[1]);
        assert_eq!(stored.strides, plan.strides);
        let baseline_root = tempfile::tempdir().unwrap();
        let baseline = fixtures::database(baseline_root.path(), &plan);
        fixtures::baseline_operation(&baseline, &plan.id, "canvas_note", |aggregate| {
            aggregate.elements.push(CanvasElement {
                id: "baseline".into(),
                kind: "note".into(),
                canvas: ElementCanvas::default(),
                content: serde_json::json!({"text":"note"}),
                links: vec![],
            });
        });
    }
}

#[test]
fn failed_canvas_delete_keeps_the_element_and_revision_durable() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "canvas".into(),
        root.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.elements.push(CanvasElement {
        id: "element".into(),
        kind: "note".into(),
        canvas: Default::default(),
        content: serde_json::json!({"text":"note"}),
        links: vec![],
    });
    drop(fixtures::database(root.path(), &plan));
    let store = PlanStore::open(root.path().join("plans.db")).unwrap();
    store.database.pending_changes.lock().take(&plan.id);
    store.database.connection.lock().execute_batch("CREATE TRIGGER reject_canvas_revision BEFORE UPDATE ON plans WHEN json_extract(NEW.header_json,'$._revision') != coalesce(json_extract(OLD.header_json,'$._revision'),0) BEGIN SELECT RAISE(ABORT,'injected canvas commit failure'); END;").unwrap();
    assert!(store
        .rm_element(&plan.id, "element", 20)
        .unwrap_err()
        .to_string()
        .contains("injected canvas commit failure"));
    assert!(store.take_committed_change(&plan.id).is_none());
    crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&store.database);
    drop(store);
    let reopened = PlanDatabase::open(root.path().join("plans.db")).unwrap();
    assert_eq!(reopened.load_plan(&plan.id).unwrap().unwrap(), plan);
}

#[test]
fn concurrent_canvas_edits_preserve_independent_element_fields() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "canvas".into(),
        root.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.elements.push(CanvasElement {
        id: "element".into(),
        kind: "note".into(),
        canvas: Default::default(),
        content: serde_json::json!({"text":"note"}),
        links: vec![],
    });
    drop(fixtures::database(root.path(), &plan));
    let store = Arc::new(PlanStore::open(root.path().join("plans.db")).unwrap());
    let barrier = Arc::new(Barrier::new(3));
    let handles: Vec<_> = [true, false]
        .into_iter()
        .map(|kind| {
            let store = store.clone();
            let barrier = barrier.clone();
            let id = plan.id.clone();
            std::thread::spawn(move || {
                barrier.wait();
                if kind {
                    store
                        .update_element(&id, "element", Some("changed"), None, None, 20)
                        .unwrap();
                } else {
                    store
                        .update_element(
                            &id,
                            "element",
                            None,
                            Some(serde_json::json!({"text":"edited"})),
                            None,
                            20,
                        )
                        .unwrap();
                }
            })
        })
        .collect();
    barrier.wait();
    for handle in handles {
        handle.join().unwrap();
    }
    let stored = store.database.load_plan(&plan.id).unwrap().unwrap();
    assert_eq!(stored.elements[0].kind, "changed");
    assert_eq!(
        stored.elements[0].content,
        serde_json::json!({"text":"edited"})
    );
}
