use super::*;
use crate::plan::{EffortLink, EffortLinkRel, Spec, TaskStatus};

fn decorate(plan: &mut Plan, root: &std::path::Path, extra: usize) -> (Plan, Spec) {
    plan.sections = sections();
    plan.tasks[0].section_ids = vec!["sec_b".into(), "removed_section".into()];
    plan.tasks[1].parent_task_id = Some(plan.tasks[0].id.clone());
    let mut ancestor = Plan::new(
        "Ancestor".into(),
        root.display().to_string(),
        OrchestratorRef::default(),
    );
    ancestor.id = "pl_ancestor".into();
    plan.lineage = vec![
        EffortLink {
            to: ancestor.id.clone(),
            rel: EffortLinkRel::Follows,
        },
        EffortLink {
            to: "removed_plan".into(),
            rel: EffortLinkRel::Follows,
        },
    ];
    let mut spec = Spec::new("Spec".into(), root.display().to_string());
    spec.id = "spec_related".into();
    spec.sections = sections();
    spec.sections[0].body = "s".repeat(32768 + extra);
    plan.spec_links = serde_json::from_value(json!([
        {"plan_section_id":"sec_b","spec_id":spec.id,"spec_section_id":"sec_b"},
        {"plan_section_id":"sec_b","spec_id":spec.id,"spec_section_id":"removed_spec_section"},
        {"plan_section_id":"sec_b","spec_id":spec.id,"spec_section_id":"sec_b"}
    ]))
    .unwrap();
    (ancestor, spec)
}

#[test]
fn related_sql_matches_tree_order_scoped_labels_lineage_and_removed_units() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Related".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.tasks = vec![
        crate::service_plan::load_audit::task("one", TaskStatus::Pending, None),
        crate::service_plan::load_audit::task("child", TaskStatus::Pending, None),
    ];
    let (ancestor, spec) = decorate(&mut plan, directory.path(), 0);
    let database = fixtures::database(directory.path(), &plan);
    database.persist_plan_rows(&ancestor, None).unwrap();
    database.persist_spec(&spec).unwrap();
    for id in [plan.id.as_str(), "one", "child", "sec_a", "sec_b", "absent"] {
        let actual = database.related_projection(id).unwrap();
        let expected = crate::service_plan::search_related::related_units(
            &[plan.clone(), ancestor.clone()],
            &[spec.clone()],
            id,
        );
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap(),
            "id={id}"
        );
    }
    let actual = database.related_projection("sec_b").unwrap();
    assert_eq!(
        actual[1]
            .units
            .iter()
            .map(|unit| unit.removed)
            .collect::<Vec<_>>(),
        vec![false, true, false]
    );
    database
        .connection
        .lock()
        .execute("DELETE FROM plans WHERE id=?1", [&ancestor.id])
        .unwrap();
    database
        .connection
        .lock()
        .execute("DELETE FROM specs WHERE id=?1", [&spec.id])
        .unwrap();
    let actual = database.related_projection("sec_b").unwrap();
    assert!(actual[1].units.iter().all(|unit| unit.removed));
    assert!(actual[2].units.iter().all(|unit| unit.removed));
}

#[test]
fn related_materializes_only_output_labels_when_unrelated_bodies_and_history_expand() {
    let mut prior = None;
    for (strides, attempts, extra_body) in [(0, 1, 0), (2048, 1, 0), (0, 2048, 0), (0, 1, 32768)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.tasks[2].body.push_str(&"x".repeat(extra_body));
        let (ancestor, spec) = decorate(&mut plan, directory.path(), extra_body);
        let database = fixtures::database(directory.path(), &plan);
        database.persist_plan_rows(&ancestor, None).unwrap();
        database.persist_spec(&spec).unwrap();
        fixtures::audit(&database);
        super::super::super::super::store_sync::take_decoded_row_bytes();
        let full = database.load_plan(&plan.id).unwrap().unwrap();
        let baseline_decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        take_metrics();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let mut responses = Vec::new();
        for id in ["task_0000", "sec_b"] {
            let actual = database.related_projection(id).unwrap();
            let expected = crate::service_plan::search_related::related_units(
                &[full.clone(), ancestor.clone()],
                &[spec.clone()],
                id,
            );
            assert_eq!(
                serde_json::to_value(&actual).unwrap(),
                serde_json::to_value(expected).unwrap()
            );
            responses.push(actual);
        }
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        let response_bytes = serde_json::to_vec(&responses).unwrap().len();
        assert!(metrics.materialized_bytes < 4096);
        assert!(decoded < 4096);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=related tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={extra_body} baseline_decoded_json_bytes={baseline_decoded} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response_bytes} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response_bytes);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}

#[test]
fn related_reads_coherent_task_and_lineage_commits_and_propagates_bad_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Related".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.tasks = vec![
        crate::service_plan::load_audit::task("one", TaskStatus::Pending, None),
        crate::service_plan::load_audit::task("child", TaskStatus::Pending, None),
    ];
    let (mut ancestor, spec) = decorate(&mut plan, directory.path(), 0);
    plan.tasks[0].name = "A".into();
    ancestor.title = "A".into();
    let database = fixtures::database(directory.path(), &plan);
    database.persist_plan_rows(&ancestor, None).unwrap();
    database.persist_spec(&spec).unwrap();
    let path = database.path();
    let id = plan.id.clone();
    let ancestor_id = ancestor.id.clone();
    let barrier = Arc::new(Barrier::new(2));
    let writer_barrier = barrier.clone();
    let writer = std::thread::spawn(move || {
        let mut connection = rusqlite::Connection::open(path).unwrap();
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        for index in 0..16 {
            let name = if index % 2 == 0 { "B" } else { "A" };
            let tx = connection.transaction().unwrap();
            tx.execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.name',?2) WHERE plan_id=?1 AND task_id='one'",params![id,name]).unwrap();
            tx.execute("UPDATE plans SET title=?2,header_json=json_set(header_json,'$.title',?2) WHERE id=?1",params![ancestor_id,name]).unwrap();
            writer_barrier.wait();
            tx.commit().unwrap();
            writer_barrier.wait();
        }
    });
    for _ in 0..16 {
        barrier.wait();
        let groups = database.related_projection("sec_b").unwrap();
        assert_eq!(groups[0].units[0].title, groups[2].units[0].title);
        barrier.wait();
    }
    writer.join().unwrap();
    database
        .connection
        .lock()
        .execute(
            "UPDATE plans SET header_json=json_set(header_json,'$.lineage','invalid') WHERE id=?1",
            [&plan.id],
        )
        .unwrap();
    assert!(matches!(
        database.related_projection("one"),
        Err(PlanError::Quarantined { .. })
    ));
}
