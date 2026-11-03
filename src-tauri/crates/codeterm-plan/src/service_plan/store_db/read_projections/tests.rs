use super::*;
use crate::plan::{OrchestratorRef, Plan};
use crate::service_plan::store_db::hot_write::tests::fixtures;
use crate::service_plan::PlanStore;
use std::sync::{Arc, Barrier};

fn sections() -> Vec<Section> {
    serde_json::from_value(json!([
        {"id":"sec_a","title":"Root","body":"root prose","subsections":[
            {"id":"sec_b","handle":"nested","title":"Nested","body":"selected prose","subsections":[
                {"id":"sec_c","title":"Leaf","body":"leaf prose"}
            ]},
            {"id":"sec_d","title":"Sibling","body":"sibling prose"}
        ]},
        {"id":"sec_e","title":"Other","body":"other prose"}
    ]))
    .unwrap()
}

#[test]
fn section_selection_preserves_tree_order_and_subtree_output() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Outline".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.sections = sections();
    let database = fixtures::database(directory.path(), &plan);
    let outline = database.section_outline(&plan.id).unwrap();
    assert_eq!(
        outline,
        json!({"id":plan.id,"title":"Outline","sections":[
            {"id":"sec_a","title":"Root","subsections":[
                {"id":"sec_b","title":"Nested","subsections":[{"id":"sec_c","title":"Leaf","subsections":[]}]},
                {"id":"sec_d","title":"Sibling","subsections":[]}
            ]}, {"id":"sec_e","title":"Other","subsections":[]}
        ]})
    );
    assert_eq!(
        database.selected_section(&plan.id, "nested").unwrap(),
        plan.sections[0].subsections[0]
    );
    assert_eq!(
        database.selected_section(&plan.id, "sec_b").unwrap(),
        plan.sections[0].subsections[0]
    );
    assert!(matches!(
        database.selected_section(&plan.id, "absent"),
        Err(PlanError::NotFound)
    ));
    assert!(matches!(
        database.section_outline("absent"),
        Err(PlanError::NotFound)
    ));
}

#[test]
fn section_and_reference_reads_do_not_materialize_unrelated_body_or_history() {
    let mut previous = None;
    for (strides, attempts, extra_body) in [(0, 0, 0), (2048, 0, 0), (0, 2048, 0), (0, 0, 32768)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.sections = sections();
        plan.sections[1].body = "x".repeat(extra_body);
        plan.tasks[1].body.push_str(&"x".repeat(extra_body));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        super::super::super::store_sync::take_decoded_row_bytes();
        let baseline = database.load_plan(&plan.id).unwrap().unwrap();
        let baseline_decoded = super::super::super::store_sync::take_decoded_row_bytes();
        let baseline_serialized = serde_json::to_vec(&baseline).unwrap().len();
        assert!(baseline_decoded > 31 * 1024 * 1024);
        println!("baseline=whole_plan fixture={} strides={strides} unrelated_attempts={attempts} unrelated_extra_body={extra_body} decoded_json_bytes={baseline_decoded} response_bytes={baseline_serialized}", fixtures::SEED);
        drop(baseline);
        take_metrics();
        super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::store_rows::take_serialized_row_bytes();
        let outline = database.section_outline(&plan.id).unwrap();
        let selected = database.selected_section(&plan.id, "nested").unwrap();
        let references = database.scoped_task_references(&plan.id).unwrap();
        let section_refs = database.scoped_section_references(&plan.id).unwrap();
        let metrics = take_metrics();
        let decoded = super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::store_rows::take_serialized_row_bytes();
        let response_bytes = serde_json::to_vec(&(outline, selected)).unwrap().len();
        assert_eq!(references.len(), fixtures::TASKS);
        assert_eq!(section_refs.len(), 5);
        assert_eq!(metrics.queries, 5);
        assert!(metrics.materialized_bytes < 100_000);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} tasks={} base_bytes={} strides={strides} unrelated_attempts={attempts} unrelated_extra_body={extra_body} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response_bytes} changed_rows=0", fixtures::SEED, fixtures::TASKS, fixtures::CONTENT_BYTES, metrics.queries, metrics.materialized_bytes);
        let measured = (metrics, decoded, response_bytes);
        if let Some(previous) = &previous {
            assert_eq!(&measured, previous);
        }
        previous = Some(measured);
    }
}

#[test]
fn scoped_resolvers_keep_case_precedence_ambiguity_and_deletion() {
    let directory = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(directory.path());
    let mut plan = Plan::new(
        "References".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.sections = sections();
    let mut first =
        crate::service_plan::load_audit::task("task_alpha", crate::plan::TaskStatus::Pending, None);
    first.handle = Some("named".into());
    first.key = Some("COD-1".into());
    let mut second = first.clone();
    second.id = "task_alpine".into();
    second.handle = Some("COD-1".into());
    second.key = Some("COD-2".into());
    plan.tasks = vec![first, second];
    store.save(&mut plan, 1).unwrap();
    crate::service_plan::load_audit::start(&store);
    assert_eq!(
        store
            .resolve_task_ref(Some(&plan.id), "NAMED")
            .unwrap()
            .task_id,
        "task_alpha"
    );
    assert!(store.resolve_task_ref(Some(&plan.id), "task_al").is_err());
    assert!(store.resolve_task_ref(Some(&plan.id), "cod-1").is_err());
    assert_eq!(
        store.task_display_key(&plan.id, "task_alpha").as_deref(),
        Some("COD-1")
    );
    assert!(store.task_display_key(&plan.id, "task_al").is_none());
    assert!(store.task_display_key(&plan.id, "cod-1").is_none());
    assert_eq!(
        store
            .resolve_section_ref(Some(&plan.id), "nested")
            .unwrap()
            .section_id,
        "sec_b"
    );
    assert!(store.plan_loads_for_test().is_empty());
    assert!(store.cached_plan_ids_for_test().is_empty());
    store.delete(&plan.id).unwrap();
    assert!(store.resolve_task_ref(Some(&plan.id), "NAMED").is_err());
    assert!(store.show_section(&plan.id, "nested").is_err());
}

#[test]
fn section_outline_reads_one_committed_snapshot_during_concurrent_commits() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "A".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.sections = serde_json::from_value(json!([{"id":"sec_a","title":"A"}])).unwrap();
    let database = Arc::new(fixtures::database(directory.path(), &plan));
    let barrier = Arc::new(Barrier::new(2));
    let writer_barrier = barrier.clone();
    let path = database.path();
    let id = plan.id.clone();
    let writer = std::thread::spawn(move || {
        let mut connection = rusqlite::Connection::open(path).unwrap();
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        for index in 0..32 {
            let title = if index % 2 == 0 { "B" } else { "A" };
            let transaction = connection.transaction().unwrap();
            transaction
                .execute(
                    "UPDATE plans SET title = ?2 WHERE id = ?1",
                    params![id, title],
                )
                .unwrap();
            transaction.execute("UPDATE plan_sections SET row_json = json_set(row_json, '$.title', ?2) WHERE plan_id = ?1", params![id, title]).unwrap();
            writer_barrier.wait();
            transaction.commit().unwrap();
            writer_barrier.wait();
        }
    });
    for _ in 0..32 {
        barrier.wait();
        let outline = database.section_outline(&plan.id).unwrap();
        assert_eq!(outline["title"], outline["sections"][0]["title"]);
        barrier.wait();
    }
    writer.join().unwrap();
    assert_eq!(database.section_outline(&plan.id).unwrap()["title"], "A");
}
