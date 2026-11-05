use super::*;

fn decorate(plan: &mut Plan) {
    plan.sections = sections();
    plan.tasks[0].acceptance_criteria = vec!["Required result".into(), "Unicode λ".into()];
    plan.tasks[0].external_refs=serde_json::from_value(json!([
        {"kind":"github_issue","repository":"owner/project","number":42,"canonical_url":"https://github.com/owner/project/issues/42"}
    ])).unwrap();
    plan.tasks[0].milestones = serde_json::from_value(
        json!([{ "id":"indexed","from":{},"kind":"decision","text":"Indexed text","ts":1 }]),
    )
    .unwrap();
    plan.tasks[0].milestones[0].ts = u64::MAX;
}

#[test]
fn search_snapshot_matches_index_output_without_attempt_or_plan_stride_materialization() {
    let mut prior = None;
    for (strides, attempts, extra) in [(0, 1, 0), (2048, 1, 0), (0, 2048, 0), (0, 1, 32768)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        decorate(&mut plan);
        plan.tasks[2].test_plan = Some("x".repeat(extra));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        super::super::super::super::store_sync::take_decoded_row_bytes();
        let full = database.load_plan(&plan.id).unwrap().unwrap();
        let baseline = super::super::super::super::store_sync::take_decoded_row_bytes();
        take_metrics();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let snapshot = database.search_snapshot(&plan.id).unwrap();
        let response_bytes = snapshot.assert_matches(&full);
        snapshot.assert_section_ordinals_for_test(&[
            ("sec_a", "Root", 1),
            ("sec_b", "Nested", 2),
            ("sec_c", "Leaf", 3),
            ("sec_d", "Sibling", 4),
            ("sec_e", "Other", 5),
        ]);
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        assert_eq!(metrics.queries, 4);
        assert!(metrics.materialized_bytes < response_bytes);
        assert!(decoded < response_bytes);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=search_recovery tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_unindexed_text={extra} baseline_decoded_json_bytes={baseline} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} index_output_bytes={response_bytes} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response_bytes);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior)
        }
        prior = Some(measured);
    }
}

#[test]
fn search_snapshot_keeps_header_task_section_and_milestone_commits_coherent() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "A".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.tasks = vec![crate::service_plan::load_audit::task(
        "task",
        crate::plan::TaskStatus::Pending,
        None,
    )];
    decorate(&mut plan);
    plan.sections[0].title = "A".into();
    plan.tasks[0].name = "A".into();
    plan.tasks[0].milestones[0].text = "A".into();
    let database = fixtures::database(directory.path(), &plan);
    let path = database.path();
    let id = plan.id.clone();
    let barrier = Arc::new(Barrier::new(2));
    let writer_barrier = barrier.clone();
    let writer = std::thread::spawn(move || {
        let mut connection = rusqlite::Connection::open(path).unwrap();
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        for index in 0..16 {
            let text = if index % 2 == 0 { "B" } else { "A" };
            let tx = connection.transaction().unwrap();
            tx.execute("UPDATE plans SET title=?2,header_json=json_set(header_json,'$.title',?2) WHERE id=?1",params![id,text]).unwrap();
            tx.execute(
                "UPDATE plan_tasks SET row_json=json_set(row_json,'$.name',?2) WHERE plan_id=?1",
                params![id, text],
            )
            .unwrap();
            tx.execute("UPDATE plan_sections SET row_json=json_set(row_json,'$.title',?2) WHERE plan_id=?1 AND position=0",params![id,text]).unwrap();
            tx.execute("UPDATE plan_task_milestones SET row_json=json_set(row_json,'$.text',?2) WHERE plan_id=?1",params![id,text]).unwrap();
            writer_barrier.wait();
            tx.commit().unwrap();
            writer_barrier.wait();
        }
    });
    for _ in 0..16 {
        barrier.wait();
        let snapshot = database.search_snapshot(&plan.id).unwrap();
        snapshot.assert_coherent_test_labels();
        barrier.wait();
    }
    writer.join().unwrap();
    database
        .connection
        .lock()
        .execute(
            "UPDATE plan_tasks SET row_json=json_set(row_json,'$.name',NULL) WHERE plan_id=?1",
            [&plan.id],
        )
        .unwrap();
    assert!(matches!(
        database.search_snapshot(&plan.id),
        Err(PlanError::Quarantined { .. })
    ));
    assert!(matches!(
        database.search_snapshot("absent"),
        Err(PlanError::NotFound)
    ));
}
