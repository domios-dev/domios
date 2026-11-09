use super::*;
use crate::plan::Section;
use crate::service_plan::store_db::hot_write::tests::fixtures;
use crate::service_plan::store_db::read_projections::take_metrics;

fn sections() -> Vec<Section> {
    serde_json::from_value(serde_json::json!([
        {"id":"root","title":"root","subsections":[{"id":"nested","title":"nested","handle":"alias","subsections":[{"id":"leaf","title":"leaf"}]},{"id":"sibling","title":"sibling"}]},
        {"id":"other","title":"other","handle":"alias"}
    ])).unwrap()
}

#[test]
fn section_removal_selects_first_matching_subtree_ids_in_preorder() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(directory.path(), 0, 0);
    plan.sections = sections();
    let database = fixtures::database(directory.path(), &plan);
    for (selector, expected) in [
        ("root", vec!["root", "nested", "leaf", "sibling"]),
        ("alias", vec!["nested", "leaf"]),
        ("nested", vec!["nested", "leaf"]),
        ("sibling", vec!["sibling"]),
        ("missing", vec![]),
    ] {
        assert_eq!(
            database
                .section_removal_ids_projection(&plan.id, selector)
                .unwrap(),
            expected
        );
    }
    assert!(matches!(
        database.section_removal_ids_projection("missing", "root"),
        Err(PlanError::NotFound)
    ));
    database
        .connection
        .lock()
        .execute_batch("DROP TABLE plan_sections")
        .unwrap();
    assert!(database
        .section_removal_ids_projection(&plan.id, "root")
        .is_err());
}

#[test]
fn rm_section_fixture_selects_only_attachment_ids_without_serialization_or_writes() {
    let mut prior = None;
    for (strides, attempts, body) in [(0, 1, 0), (2048, 1, 0), (0, 2048, 0), (0, 1, 32768)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.sections = sections();
        plan.sections[0].body = "x".repeat(body);
        plan.tasks[0].body.push_str(&"x".repeat(body));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let ids = database
            .section_removal_ids_projection(&plan.id, "alias")
            .unwrap();
        assert_eq!(ids, vec!["nested", "leaf"]);
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&ids).unwrap().len();
        assert_eq!(metrics.queries, 2);
        assert!(metrics.materialized_bytes < 128);
        assert_eq!(decoded, 0);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=rm_section tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}
