use super::*;

fn decorate(plan: &mut Plan, extra: usize) {
    plan.gates = serde_json::from_value(json!([
        {"id":"closed","question":"Answered","asked_at":1,"status":"answered"},
        {"id":"open","question":"Question","asked_at":2,"status":"open"},
        {"id":"escalated","question":"Escalated","asked_at":3,"status":"escalated"}
    ]))
    .unwrap();
    plan.elements = serde_json::from_value(json!([
        {"id":"note","kind":"note","content":{"title":"\u{2003}","name":42,"text":format!("\n First line \n{}","x".repeat(extra))}},
        {"id":"first","kind":"handoff","content":{"seq":7,"mode":"compact","goal":"Goal","ts":u64::MAX}},
        {"id":"7","kind":"handoff","content":{"seq":u64::MAX,"mode":42,"goal":false}},
        {"id":"last","kind":"handoff","content":{"seq":u64::MAX}},
        {"id":"invalid","kind":"handoff","content":{"seq":"7","ts":1.5}}
    ])).unwrap();
}

#[test]
fn auxiliary_queries_preserve_filters_titles_and_handoff_selector_ties() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Auxiliary".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    decorate(&mut plan, 32);
    let database = fixtures::database(directory.path(), &plan);
    assert_eq!(
        database.gate_rows(&plan.id, false, None).unwrap(),
        plan.gates
    );
    assert_eq!(
        database.gate_rows(&plan.id, true, None).unwrap(),
        plan.gates[1..]
    );
    assert_eq!(
        database.gate_rows(&plan.id, false, Some("closed")).unwrap(),
        plan.gates[..1]
    );
    assert!(database
        .gate_rows(&plan.id, false, Some("absent"))
        .unwrap()
        .is_empty());
    let summaries = database.element_summaries(&plan.id, None).unwrap();
    let expected = plan
        .elements
        .iter()
        .map(crate::service_plan::element_queries::element_summary)
        .collect::<Vec<_>>();
    assert_eq!(
        serde_json::to_value(summaries).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert_eq!(
        database.element_summaries(&plan.id, Some("note")).unwrap()[0].title,
        "First line"
    );
    assert_eq!(
        database.selected_element(&plan.id, "note").unwrap(),
        plan.elements[0]
    );
    assert_eq!(
        database.selected_handoff(&plan.id, Some("7")).unwrap().1.id,
        "first"
    );
    assert_eq!(
        database.selected_handoff(&plan.id, None).unwrap().1.id,
        "last"
    );
    assert_eq!(
        database.selected_handoff(&plan.id, Some("0")).unwrap().1.id,
        "invalid"
    );
    assert_eq!(
        database
            .selected_handoff(&plan.id, Some("18446744073709551615"))
            .unwrap()
            .1
            .id,
        "7"
    );
    let handoffs = database.handoff_metadata(&plan.id).unwrap();
    assert_eq!(handoffs[0].content["ts"].as_u64(), Some(u64::MAX));
    assert!(handoffs[1].content["mode"].is_null());
    assert!(matches!(
        database.selected_element(&plan.id, "absent"),
        Err(PlanError::ElementNotFound)
    ));
    assert!(matches!(
        database.selected_handoff("absent", None),
        Err(PlanError::NotFound)
    ));
    database.connection.lock().execute("UPDATE plan_elements SET row_json=json_set(row_json,'$.links',NULL) WHERE plan_id=?1 AND element_id='note'",[&plan.id]).unwrap();
    assert!(matches!(
        database.element_summaries(&plan.id, None),
        Err(PlanError::Quarantined { .. })
    ));
}

#[test]
fn auxiliary_fixed_output_ignores_unrelated_task_history_and_element_prose() {
    let mut prior = None;
    for (strides, attempts, extra) in [(0, 1, 0), (2048, 1, 0), (0, 2048, 0), (0, 1, 32768)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        decorate(&mut plan, extra);
        plan.tasks[2].body.push_str(&"x".repeat(extra));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        super::super::super::super::store_sync::take_decoded_row_bytes();
        let full = database.load_plan(&plan.id).unwrap().unwrap();
        let baseline = super::super::super::super::store_sync::take_decoded_row_bytes();
        take_metrics();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let gates = database.gate_rows(&plan.id, true, None).unwrap();
        let elements = database.element_summaries(&plan.id, None).unwrap();
        let handoffs = database.handoff_metadata(&plan.id).unwrap();
        let (_, selected) = database.selected_handoff(&plan.id, None).unwrap();
        // Directory spellings vary across temporary fixtures; only query bytes enter the bound.
        let response = json!([gates, elements, handoffs, selected]);
        assert_eq!(response[1][0]["title"], "First line");
        assert_eq!(
            response[3],
            serde_json::to_value(&full.elements[3]).unwrap()
        );
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        let response_bytes = serde_json::to_vec(&response).unwrap().len();
        assert!(metrics.materialized_bytes < 8192);
        assert!(decoded < 8192);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=auxiliary tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={extra} baseline_decoded_json_bytes={baseline} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response_bytes} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response_bytes);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior)
        }
        prior = Some(measured);
    }
}

#[test]
fn handoff_storage_and_selected_content_share_committed_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Snapshot".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    decorate(&mut plan, 0);
    plan.canonical_project_path.clear();
    plan.project_path = directory.path().join("A").display().to_string();
    plan.elements[3].content["root"] = json!("A");
    let database = fixtures::database(directory.path(), &plan);
    let path = database.path();
    let id = plan.id.clone();
    let root = directory.path().to_path_buf();
    let barrier = Arc::new(Barrier::new(2));
    let writer_barrier = barrier.clone();
    let writer = std::thread::spawn(move || {
        let mut connection = rusqlite::Connection::open(path).unwrap();
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        for index in 0..16 {
            let name = if index % 2 == 0 { "B" } else { "A" };
            let project = root.join(name).display().to_string();
            let tx = connection.transaction().unwrap();
            tx.execute("UPDATE plans SET project_path=?2,header_json=json_set(header_json,'$.project_path',?2) WHERE id=?1",params![id,project]).unwrap();
            tx.execute("UPDATE plan_elements SET row_json=json_set(row_json,'$.content.root',?2) WHERE plan_id=?1 AND element_id='last'",params![id,name]).unwrap();
            writer_barrier.wait();
            tx.commit().unwrap();
            writer_barrier.wait();
        }
    });
    for _ in 0..16 {
        barrier.wait();
        let (metadata, element) = database.selected_handoff(&plan.id, None).unwrap();
        let mut expected = plan.clone();
        expected.project_path = directory
            .path()
            .join(element.content["root"].as_str().unwrap())
            .display()
            .to_string();
        assert_eq!(
            metadata.state_dir().unwrap(),
            crate::service_plan::body_files::plan_body_dir(&expected).unwrap()
        );
        barrier.wait();
    }
    writer.join().unwrap();
    database.connection.lock().execute("UPDATE plans SET header_json=json_set(header_json,'$.canonical_project_path',NULL) WHERE id=?1",[&plan.id]).unwrap();
    assert!(matches!(
        database.selected_handoff(&plan.id, None),
        Err(PlanError::Quarantined { .. })
    ));
}
