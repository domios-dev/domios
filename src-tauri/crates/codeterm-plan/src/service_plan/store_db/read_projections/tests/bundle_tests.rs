use super::*;
use crate::plan::coverage::{compute_spec_coverage_report, PlanCoverageInput};
use crate::plan::{Spec, SpecLink, TaskStatus};

fn linked_spec(plan: &mut Plan) -> Spec {
    plan.sections = sections();
    plan.tasks[0].section_ids = vec!["sec_a".into()];
    let mut spec = Spec::new("A".into(), plan.project_path.clone());
    spec.id = "spec_selected".into();
    spec.created_at = 1;
    spec.updated_at = 2;
    spec.sections =
        serde_json::from_value(json!([{"id":"req","title":"A","body":"requested spec prose"}]))
            .unwrap();
    plan.spec_refs = vec![spec.id.clone()];
    plan.spec_links = vec![SpecLink {
        spec_id: spec.id.clone(),
        spec_section_id: "req".into(),
        plan_section_id: "sec_a".into(),
    }];
    spec
}

#[test]
fn bundle_exports_preserve_documents_coverage_reference_order_and_loud_errors() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "A".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.tasks = vec![crate::service_plan::load_audit::task(
        "selected",
        TaskStatus::Done,
        None,
    )];
    let spec = linked_spec(&mut plan);
    let mut second = spec.clone();
    second.id = "spec_second".into();
    second.title = "Second".into();
    plan.spec_refs = vec![
        second.id.clone(),
        "missing".into(),
        spec.id.clone(),
        second.id.clone(),
    ];
    let database = fixtures::database(directory.path(), &plan);
    database.persist_spec(&spec).unwrap();
    database.persist_spec(&second).unwrap();
    let store = PlanStore::open(database.path()).unwrap();
    fixtures::audit(&database);
    let inputs = [PlanCoverageInput::from(&plan)];
    let first_report = compute_spec_coverage_report(&spec, &inputs);
    let second_report = compute_spec_coverage_report(&second, &inputs);
    assert_eq!(
        store.spec_full(&spec.id).unwrap().unwrap(),
        json!({
            "spec":spec,"coverage":first_report,"links":[[plan.id,plan.spec_links[0]]]
        })
    );
    assert_eq!(
        store.effort_bundle(&plan.id).unwrap().unwrap(),
        json!({
            "plan":plan,"specs":[second,spec,second],"spec_coverage":[
                {"spec_id":second.id,"coverage":second_report},
                {"spec_id":spec.id,"coverage":first_report},
                {"spec_id":second.id,"coverage":second_report}
            ]
        })
    );
    assert!(store.spec_full("missing").unwrap().is_none());
    assert!(store.effort_bundle("missing").unwrap().is_none());
    assert!(store.plan_loads_for_test().is_empty());
    assert!(fixtures::audit_counts(&database).is_empty());
    database
        .connection
        .lock()
        .execute(
            "UPDATE specs SET row_json=json_set(row_json,'$.status','invalid') WHERE id=?1",
            [&second.id],
        )
        .unwrap();
    assert!(store.spec_full(&spec.id).unwrap().is_some());
    assert!(matches!(
        store.spec_full(&second.id),
        Err(PlanError::Quarantined { .. })
    ));
    assert!(matches!(
        store.effort_bundle(&plan.id),
        Err(PlanError::Quarantined { .. })
    ));
}

#[test]
fn spec_full_materialization_ignores_linked_plan_bodies_and_history_growth() {
    let mut prior = None;
    for (strides, attempts, body, rounds) in [
        (0, 1, 0, 0),
        (2048, 1, 0, 0),
        (0, 2048, 0, 0),
        (0, 1, 32768, 0),
        (0, 1, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        let spec = linked_spec(&mut plan);
        plan.tasks[1].body.push_str(&"x".repeat(body));
        plan.strides.extend(fixtures::round_strides(
            (0..rounds)
                .map(|number| crate::plan::CuratorRound {
                    number,
                    ..Default::default()
                })
                .collect(),
        ));
        let database = fixtures::database(directory.path(), &plan);
        database.persist_spec(&spec).unwrap();
        fixtures::audit(&database);
        crate::service_plan::store_sync::take_decoded_row_bytes();
        database.load_plan(&plan.id).unwrap().unwrap();
        let baseline = crate::service_plan::store_sync::take_decoded_row_bytes();
        take_metrics();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let result = database.spec_full(&spec.id).unwrap().unwrap();
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&result).unwrap().len();
        assert_eq!(metrics.queries, 5);
        assert!(metrics.materialized_bytes < 8192);
        assert!(decoded < 8192);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=spec_full tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} rounds={rounds} baseline_decoded_json_bytes={baseline} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}

#[test]
fn spec_and_effort_bundle_exports_share_document_and_coverage_snapshots() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "A".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.tasks = vec![crate::service_plan::load_audit::task(
        "selected",
        TaskStatus::Pending,
        None,
    )];
    let spec = linked_spec(&mut plan);
    let database = fixtures::database(directory.path(), &plan);
    database.persist_spec(&spec).unwrap();
    let path = database.path();
    let barrier = Arc::new(Barrier::new(2));
    let writer_barrier = barrier.clone();
    let writer = std::thread::spawn(move || {
        let mut connection = rusqlite::Connection::open(path).unwrap();
        connection
            .busy_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        for index in 0..16 {
            let (title, status) = if index % 2 == 0 {
                ("B", "done")
            } else {
                ("A", "pending")
            };
            let tx = connection.transaction().unwrap();
            tx.execute(
                "UPDATE plans SET title=?1,header_json=json_set(header_json,'$.title',?1)",
                [title],
            )
            .unwrap();
            tx.execute(
                "UPDATE plan_tasks SET row_json=json_set(row_json,'$.status',?1)",
                [status],
            )
            .unwrap();
            tx.execute("UPDATE specs SET row_json=json_set(row_json,'$.title',?1,'$.sections[0].title',?1) WHERE id='spec_selected'",[title]).unwrap();
            writer_barrier.wait();
            tx.commit().unwrap();
            writer_barrier.wait();
        }
    });
    for _ in 0..16 {
        barrier.wait();
        let full = database.spec_full(&spec.id).unwrap().unwrap();
        assert_eq!(
            full["spec"]["title"],
            full["coverage"]["sections"][0]["title"]
        );
        assert_eq!(
            full["coverage"]["sections"][0]["done"],
            usize::from(full["spec"]["title"] == "B")
        );
        let bundle = database.effort_bundle(&plan.id).unwrap().unwrap();
        assert_eq!(bundle["plan"]["title"], bundle["specs"][0]["title"]);
        assert_eq!(
            bundle["plan"]["title"],
            bundle["spec_coverage"][0]["coverage"]["sections"][0]["title"]
        );
        assert_eq!(
            bundle["spec_coverage"][0]["coverage"]["sections"][0]["done"],
            usize::from(bundle["plan"]["title"] == "B")
        );
        barrier.wait();
    }
    writer.join().unwrap();
}
