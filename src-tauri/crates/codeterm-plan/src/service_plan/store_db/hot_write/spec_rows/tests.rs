use super::*;
use crate::plan::{OrchestratorRef, Plan, Section};
use crate::service_plan::store_db::hot_write::tests::fixtures;
use crate::service_plan::{load_audit, PlanStore};
use std::sync::{Arc, Barrier};

fn section(id: &str) -> Section {
    Section {
        id: id.into(),
        title: id.into(),
        handle: Some(id.into()),
        tags: vec![],
        body: "body".into(),
        subsections: vec![],
        canvas: Default::default(),
        attachments: vec![],
    }
}

fn spec(root: &std::path::Path) -> Spec {
    let mut spec = Spec::new("selected".into(), root.display().to_string());
    spec.id = "selected-spec".into();
    spec.created_at = 1;
    spec.updated_at = 1;
    spec.sections.push(section("spec-section"));
    spec
}

#[test]
fn spec_family_rows_and_bytes_ignore_unrelated_plan_history_and_body_growth() {
    let mut prior = None;
    for (strides, attempts, body_bytes) in [(0, 0, 0), (2048, 0, 0), (0, 2048, 0), (0, 0, 32768)] {
        let root = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(root.path(), strides, attempts);
        plan.tasks[1].body.push_str(&"u".repeat(body_bytes));
        plan.sections.push(section("plan-section"));
        let database = fixtures::database(root.path(), &plan);
        let selected = spec(root.path());
        database.persist_spec(&selected).unwrap();
        crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&database);
        drop(database);
        let store = PlanStore::open(root.path().join("plans.db")).unwrap();
        fixtures::audit(&store.database);
        load_audit::start(&store);
        let mut work = Vec::new();
        store
            .update_spec(&selected.id, Some("renamed"), None, 20)
            .unwrap();
        work.push(super::super::take_work());
        store
            .link_spec_section(&plan.id, &selected.id, "spec-section", "plan-section", 21)
            .unwrap();
        work.push(super::super::take_work());
        assert_eq!(
            store.get_spec(&selected.id).unwrap().plan_id.as_deref(),
            Some(plan.id.as_str())
        );
        store
            .unlink_spec_section(&plan.id, &selected.id, None, None, 22)
            .unwrap();
        work.push(super::super::take_work());
        store
            .link_spec_section(&plan.id, &selected.id, "spec-section", "plan-section", 23)
            .unwrap();
        work.push(super::super::take_work());
        store.delete_spec(&selected.id, 24).unwrap();
        work.push(super::super::take_work());
        let mut recreate = selected.clone();
        store.save_spec(&mut recreate, 25).unwrap();
        let created = store
            .create_plan_from_spec_for_team(
                &selected.id,
                "created".into(),
                OrchestratorRef::from_tab("owner"),
                "new-team".into(),
                vec![],
                26,
            )
            .unwrap();
        let creation = super::super::take_work();
        let response_bytes = serde_json::to_vec(&created).unwrap().len();
        assert!(creation[5] < 2048 && creation[6] < 8192 && response_bytes < 8192);
        println!("fixture={} tasks={} base_content_bytes={} operation=spec_family strides={strides} unrelated_attempts={attempts} unrelated_body_bytes={body_bytes} changed_rows={:?} work_task_header_attempt_milestone_stride_decoded_serialized_projected_hashed_section_element_heartbeat_spec={work:?} response_bytes=4",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,fixtures::audit_counts(&store.database));
        assert!(store.plan_loads_for_test().is_empty());
        for entry in &work {
            assert_eq!(
                (entry[0], entry[2], entry[3], entry[4], entry[9], entry[10], entry[11]),
                (0, 0, 0, 0, 0, 0, 0)
            );
            assert!(entry[5] < 2048 && entry[6] < 2048);
        }
        assert_eq!((work[0][1], work[0][12]), (0, 1));
        assert_eq!((work[1][1], work[1][12]), (4, 1));
        assert_eq!(work[4][12], 1);
        let measured = (work, creation, response_bytes);
        if let Some(prior) = prior {
            assert_eq!(prior, measured);
        }
        prior = Some(measured);
        assert_eq!(
            store.get_spec(&selected.id).unwrap().plan_id.as_deref(),
            Some(created.id.as_str())
        );
        println!("operation=create_spec_plan receipt={creation:?} response_bytes={response_bytes}");
        crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(
            &store.database,
        );
        drop(store);
        let reopened = PlanDatabase::open(root.path().join("plans.db")).unwrap();
        let stored = reopened.load_plan(&plan.id).unwrap().unwrap();
        assert!(
            stored.spec_refs.is_empty()
                && stored.spec_links.is_empty()
                && stored.primary_spec_id.is_none()
        );
        assert_eq!(stored.tasks, plan.tasks);
        assert_eq!(stored.strides, plan.strides);
        assert_eq!(
            reopened
                .load_specs()
                .unwrap()
                .into_iter()
                .find(|spec| spec.id == selected.id)
                .unwrap()
                .plan_id
                .as_deref(),
            Some(created.id.as_str())
        );
        let baseline_root = tempfile::tempdir().unwrap();
        let baseline = fixtures::database(baseline_root.path(), &plan);
        fixtures::baseline_operation(&baseline, &plan.id, "spec_link", |aggregate| {
            aggregate.spec_refs.push(selected.id.clone());
        });
    }
}

#[test]
fn failed_spec_link_rolls_back_ownership_and_every_plan_reference() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "plan".into(),
        root.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.sections.push(section("plan-section"));
    let database = fixtures::database(root.path(), &plan);
    let selected = spec(root.path());
    database.persist_spec(&selected).unwrap();
    crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&database);
    drop(database);
    let store = PlanStore::open(root.path().join("plans.db")).unwrap();
    store.database.pending_changes.lock().take(&plan.id);
    store.database.connection.lock().execute_batch("CREATE TRIGGER reject_link_revision BEFORE UPDATE ON plans WHEN json_extract(NEW.header_json,'$._revision') != coalesce(json_extract(OLD.header_json,'$._revision'),0) BEGIN SELECT RAISE(ABORT,'injected spec link failure'); END;").unwrap();
    assert!(store
        .link_spec_section(&plan.id, &selected.id, "spec-section", "plan-section", 20)
        .unwrap_err()
        .to_string()
        .contains("injected spec link failure"));
    assert!(store.take_committed_change(&plan.id).is_none());
    assert_eq!(store.get_spec(&selected.id).unwrap(), selected);
    crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&store.database);
    drop(store);
    let reopened = PlanDatabase::open(root.path().join("plans.db")).unwrap();
    assert_eq!(reopened.load_plan(&plan.id).unwrap().unwrap(), plan);
    assert_eq!(
        reopened
            .load_specs()
            .unwrap()
            .into_iter()
            .find(|spec| spec.id == selected.id)
            .unwrap(),
        selected
    );
}

#[test]
fn failed_spec_delete_rolls_back_all_referencing_plans_and_the_spec() {
    let root = tempfile::tempdir().unwrap();
    let mut first = Plan::new(
        "first".into(),
        root.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    first.id = "first".into();
    let selected = spec(root.path());
    first.spec_refs.push(selected.id.clone());
    first.primary_spec_id = Some(selected.id.clone());
    let database = fixtures::database(root.path(), &first);
    database.persist_spec(&selected).unwrap();
    let mut second = first.clone();
    second.id = "second".into();
    second.title = "second".into();
    database.persist_plan_rows(&second, None).unwrap();
    crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&database);
    drop(database);
    let store = PlanStore::open(root.path().join("plans.db")).unwrap();
    store.database.pending_changes.lock().take(&first.id);
    store.database.pending_changes.lock().take(&second.id);
    store.database.connection.lock().execute_batch("CREATE TRIGGER reject_second_plan BEFORE UPDATE ON plans WHEN NEW.id='second' BEGIN SELECT RAISE(ABORT,'injected second plan failure'); END;").unwrap();
    assert!(store
        .delete_spec(&selected.id, 20)
        .unwrap_err()
        .to_string()
        .contains("injected second plan failure"));
    assert!(
        store.take_committed_change(&first.id).is_none()
            && store.take_committed_change(&second.id).is_none()
    );
    crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&store.database);
    drop(store);
    let reopened = PlanDatabase::open(root.path().join("plans.db")).unwrap();
    assert_eq!(reopened.load_plan(&first.id).unwrap().unwrap(), first);
    assert_eq!(reopened.load_plan(&second.id).unwrap().unwrap(), second);
    assert_eq!(
        reopened
            .load_specs()
            .unwrap()
            .into_iter()
            .find(|spec| spec.id == selected.id)
            .unwrap(),
        selected
    );
}

#[test]
fn concurrent_spec_plan_creation_commits_one_claim_without_an_orphan() {
    let root = tempfile::tempdir().unwrap();
    let plan = Plan::new(
        "unrelated".into(),
        root.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    let database = fixtures::database(root.path(), &plan);
    let selected = spec(root.path());
    database.persist_spec(&selected).unwrap();
    crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&database);
    drop(database);
    let store = Arc::new(PlanStore::open(root.path().join("plans.db")).unwrap());
    let barrier = Arc::new(Barrier::new(3));
    let handles: Vec<_> = ["one", "two"]
        .into_iter()
        .map(|team| {
            let store = store.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.create_plan_from_spec_for_team(
                    "selected-spec",
                    "created".into(),
                    OrchestratorRef::from_tab("owner"),
                    team.into(),
                    vec![],
                    20,
                )
            })
        })
        .collect();
    barrier.wait();
    let results: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
    let accepted = results.into_iter().find_map(Result::ok).unwrap();
    assert_eq!(store.list_index().unwrap().len(), 2);
    assert_eq!(
        store.get_spec("selected-spec").unwrap().plan_id.as_deref(),
        Some(accepted.id.as_str())
    );
    let stored = store.database.load_plan(&accepted.id).unwrap().unwrap();
    assert_eq!(stored, accepted);
    assert_eq!(stored.spec_links.len(), 1);
    assert!(store.plan_loads_for_test().is_empty());
}

#[test]
fn failed_spec_plan_claim_rolls_back_new_plan_sections_and_owner() {
    let root = tempfile::tempdir().unwrap();
    let plan = Plan::new(
        "unrelated".into(),
        root.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    let database = fixtures::database(root.path(), &plan);
    let selected = spec(root.path());
    database.persist_spec(&selected).unwrap();
    crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&database);
    drop(database);
    let store = PlanStore::open(root.path().join("plans.db")).unwrap();
    store.database.connection.lock().execute_batch("CREATE TRIGGER reject_spec_claim BEFORE UPDATE ON specs WHEN NEW.id='selected-spec' AND NEW.plan_id IS NOT OLD.plan_id BEGIN SELECT RAISE(ABORT,'injected spec claim failure'); END;").unwrap();
    assert!(store
        .create_plan_from_spec_for_team(
            &selected.id,
            "created".into(),
            OrchestratorRef::from_tab("owner"),
            "team".into(),
            vec![],
            20
        )
        .unwrap_err()
        .to_string()
        .contains("injected spec claim failure"));
    assert_eq!(store.list_index().unwrap().len(), 1);
    crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&store.database);
    drop(store);
    let reopened = PlanDatabase::open(root.path().join("plans.db")).unwrap();
    assert_eq!(reopened.load_index_rows().unwrap().len(), 1);
    assert_eq!(
        reopened
            .load_specs()
            .unwrap()
            .into_iter()
            .find(|spec| spec.id == selected.id)
            .unwrap(),
        selected
    );
    assert_eq!(reopened.load_plan(&plan.id).unwrap().unwrap(), plan);
}
