use super::*;

#[test]
fn retrospective_presence_matches_supersede_whitespace_and_missing_semantics() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(directory.path(), 0, 1);
    let database = fixtures::database(directory.path(), &plan);
    let store = PlanStore::open(database.path()).unwrap();
    assert_eq!(
        store.retrospective_presence_projection("missing").unwrap(),
        None
    );
    for (kind, text) in [
        ("decision", "real retrospective"),
        ("retrospective", "supersedes previous retrospective"),
        (
            "retrospective",
            "\u{2003}supersedes previous retrospective\u{2009}",
        ),
        ("retrospective", ""),
        ("retrospective", "retrospective body"),
    ] {
        plan.strides = serde_json::from_value(json!([
            {"id":"retro","kind":kind,"text":text,"from":{},"ts":1}
        ]))
        .unwrap();
        database.persist_plan_rows(&plan, None).unwrap();
        assert_eq!(
            store.retrospective_presence_projection(&plan.id).unwrap(),
            Some(crate::service_plan::retrospective::has_retrospective(&plan))
        );
    }
    plan.status = crate::plan::PlanStatus::Archived;
    database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!(
        store.retrospective_presence_projection(&plan.id).unwrap(),
        Some(true)
    );
    database
        .connection
        .lock()
        .execute(
            "UPDATE plan_strides SET row_json=json_set(row_json,'$.text',42) WHERE plan_id=?1",
            [&plan.id],
        )
        .unwrap();
    assert!(store.retrospective_presence_projection(&plan.id).is_err());
    assert!(store.plan_loads_for_test().is_empty());
}

#[test]
fn retrospective_presence_fixture_returns_fixed_bytes_and_writes_nothing() {
    let mut prior = None;
    for (strides, attempts, body, history) in [
        (0, 1, 0, 0),
        (2048, 1, 0, 0),
        (0, 2048, 0, 0),
        (0, 1, 32768, 0),
        (0, 1, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        let retro = serde_json::from_value(json!({"id":"retro","kind":"retrospective","text":format!("recorded{}","x".repeat(body)),"from":{},"ts":1})).unwrap();
        plan.strides.push(retro);
        let original = plan.tasks[0].execution_attempts[0].clone();
        for index in 0..history {
            let mut old = original.clone();
            old.id = format!("closed-{index}");
            old.closed_at = Some(2);
            plan.tasks[0].execution_attempts.push(old);
        }
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.tasks[1].body.push_str(&"x".repeat(body));
        let database = fixtures::database(directory.path(), &plan);
        let store = PlanStore::open(database.path()).unwrap();
        fixtures::audit(&database);
        take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let read = store.retrospective_presence_projection(&plan.id).unwrap();
        assert_eq!(read, Some(true));
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&read).unwrap().len();
        assert_eq!(metrics.queries, 1);
        assert_eq!(metrics.materialized_bytes, 0);
        assert_eq!(decoded, 0);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        assert!(store.plan_loads_for_test().is_empty());
        println!("fixture={} consumer=retrospective_presence tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}
