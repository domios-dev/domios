use super::*;
use crate::plan::{OrchestratorRef, Plan, Section};
use crate::service_plan::store_db::hot_write::tests::fixtures;
use crate::service_plan::{load_audit, PlanStore};

fn section() -> Section {
    Section {
        id: "selected".into(),
        title: "task_0000".into(),
        handle: Some("selected".into()),
        tags: vec![],
        body: "selected body".into(),
        subsections: vec![],
        canvas: Default::default(),
        attachments: vec![],
    }
}

#[test]
fn relationships_and_regression_rows_and_bytes_ignore_unrelated_growth() {
    let mut prior = None;
    for (strides, attempts, body_bytes) in [(0, 0, 0), (2048, 0, 0), (0, 2048, 0), (0, 0, 32768)] {
        let root = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(root.path(), strides, attempts);
        plan.tasks[1].body.push_str(&"u".repeat(body_bytes));
        plan.sections.push(section());
        plan.task_groups.push(TaskGroup {
            key: "group".into(),
            label: "Group".into(),
            color: None,
        });
        let database = fixtures::database(root.path(), &plan);
        crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&database);
        drop(database);
        let store = PlanStore::open(root.path().join("plans.db")).unwrap();
        fixtures::audit(&store.database);
        load_audit::start(&store);
        let mut work = vec![];
        assert_eq!(
            store
                .link_task_sections(&plan.id, "task_0000", &["selected".into()], 20)
                .unwrap(),
            vec!["selected"]
        );
        work.push(super::super::take_work());
        assert_eq!(
            store
                .record_group_regression(
                    &plan.id,
                    "group",
                    "checked",
                    OrchestratorRef::from_tab("owner"),
                    21
                )
                .unwrap()
                .key,
            "group"
        );
        work.push(super::super::take_work());
        store
            .record_regression_override(
                &plan.id,
                vec!["group".into()],
                "authorized",
                AgentRef::from_tab("owner"),
                22,
            )
            .unwrap();
        work.push(super::super::take_work());
        let source = root.path().join("file.txt");
        std::fs::write(&source, "bytes").unwrap();
        store
            .attach_to_section(&plan.id, "selected", &source, 23)
            .unwrap();
        work.push(super::super::take_work());
        let writes = fixtures::audit_counts(&store.database);
        println!("fixture={} tasks={} base_content_bytes={} operation=relationships strides={strides} unrelated_attempts={attempts} unrelated_body_bytes={body_bytes} writes={writes:?} work={work:?} responses=selected_section_id,group_key,override,attachment",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES);
        assert!(store.plan_loads_for_test().is_empty());
        assert!(work
            .iter()
            .all(|work| work[2] == 0 && work[3] == 0 && work[5] < 65536 && work[6] < 32768));
        let measured = (work, writes);
        if let Some(prior) = prior {
            assert_eq!(prior, measured);
        }
        prior = Some(measured);
        assert!(matches!(
            store.record_regression_override(
                &plan.id,
                vec!["group".into()],
                "forged",
                AgentRef::from_tab("other"),
                24
            ),
            Err(PlanError::OverrideNotAuthorized)
        ));
        crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(
            &store.database,
        );
        drop(store);
        let reopened = PlanDatabase::open(root.path().join("plans.db")).unwrap();
        let stored = reopened.load_plan(&plan.id).unwrap().unwrap();
        assert_eq!(stored.tasks[0].section_ids, vec!["selected"]);
        assert_eq!(stored.strides.len(), plan.strides.len() + 1);
        assert_eq!(stored.regression_gate_overrides.len(), 1);
        assert_eq!(stored.sections[0].attachments.len(), 1);
        assert_eq!(stored.tasks[1], plan.tasks[1]);
    }
}

#[test]
fn relink_failure_rolls_back_every_affected_task_and_header() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "relink".into(),
        root.path().display().to_string(),
        AgentRef::from_tab("owner"),
    );
    plan.sections.push(section());
    for id in ["first", "second"] {
        let mut task = load_audit::task(id, crate::plan::TaskStatus::Pending, None);
        task.name = "task_0000".into();
        plan.tasks.push(task);
    }
    let database = fixtures::database(root.path(), &plan);
    fixtures::audit(&database);
    database.connection.lock().execute_batch("CREATE TRIGGER fail_second BEFORE UPDATE ON plan_tasks WHEN NEW.task_id='second' BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert!(database.relink_task_section_rows(&plan.id, 20).is_err());
    assert!(fixtures::audit_counts(&database).is_empty());
    let stored = database.load_plan(&plan.id).unwrap().unwrap();
    assert_eq!(stored.tasks, plan.tasks);
    assert_eq!(stored.updated_at, plan.updated_at);
    database
        .connection
        .lock()
        .execute_batch("DROP TRIGGER fail_second;")
        .unwrap();
    let (report, _) = database.relink_task_section_rows(&plan.id, 21).unwrap();
    assert_eq!(report.applied.len(), 2);
    assert_eq!(report.unlinked_count, 2);
}

#[test]
fn failed_section_attachment_removes_the_copied_file() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "attachment".into(),
        root.path().display().to_string(),
        AgentRef::from_tab("owner"),
    );
    plan.sections.push(section());
    let database = fixtures::database(root.path(), &plan);
    crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&database);
    drop(database);
    let store = PlanStore::open(root.path().join("plans.db")).unwrap();
    store.database.connection.lock().execute_batch("CREATE TRIGGER fail_section BEFORE UPDATE ON plan_sections BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    let source = root.path().join("source.txt");
    std::fs::write(&source, "bytes").unwrap();
    assert!(store
        .attach_to_section(&plan.id, "selected", &source, 20)
        .is_err());
    let directory = store
        .plan_data_dir(&plan.id)
        .join("attachments/sections/selected");
    assert!(directory
        .read_dir()
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(true));
    assert!(store.get(&plan.id).unwrap().sections[0]
        .attachments
        .is_empty());
}
