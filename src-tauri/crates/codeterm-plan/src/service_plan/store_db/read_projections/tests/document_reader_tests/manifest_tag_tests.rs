use super::*;
use crate::service_plan::{body_files, tag_model};

fn decorate_manifest_tags(plan: &mut Plan) {
    decorate(plan);
    plan.goal = Some(crate::plan::Goal {
        vector: "Manifest".into(),
        vision: "unrequested vision".into(),
        constraints: Vec::new(),
        postulates: Vec::new(),
    });
    plan.sections[0].tags = vec!["area:core".into(), "topic".into(), "topic".into()];
    plan.sections[0].subsections[0].tags = vec!["area:core".into()];
    plan.tasks[0].tags = vec!["area:core".into(), "topic".into(), "a-b".into()];
    plan.tasks[1].tags = vec!["ab".into()];
    plan.tag_facets=serde_json::from_value(json!([
        {"key":"area","label":"Area","color_lane":true,"values":[{"v":"core","label":"Core","color":"blue"},{"v":"missing"}]}
    ])).unwrap();
    plan.tasks[0].milestones=serde_json::from_value(json!([
        {"id":"unicode-wide","kind":"note","text":format!("\u{2003}{}\u{3000}","界".repeat(80)),"from":{},"ts":1},
        {"id":"existing-small","kind":"note","text":"small","from":{},"ts":1}
    ])).unwrap();
    plan.tasks[2].milestones = serde_json::from_value(json!([
        {"id":"short-only","kind":"note","text":"short","from":{},"ts":1}
    ]))
    .unwrap();
    plan.elements=serde_json::from_value(json!([
        {"id":"unicode-wide","kind":"note","content":{"text":format!("\u{2003}{}\u{3000}","界".repeat(80))}},
        {"id":"first-short","kind":"note","content":{"text":"short","body":"large ignored body ".repeat(40)}},
        {"id":"existing-custom","kind":"custom","content":{"text":"large custom body ".repeat(40)}},
        {"id":"handoff","kind":"handoff","content":{"seq":u64::MAX}}
    ])).unwrap();
}

fn existing_sidecars(plan: &Plan) {
    let paths = [
        body_files::milestone_path(plan, &plan.tasks[0], &plan.tasks[0].milestones[1]).unwrap(),
        body_files::element_path(plan, &plan.elements[2]).unwrap(),
        body_files::task_stride_log_path(plan, &plan.tasks[1]).unwrap(),
    ];
    for path in paths {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "existing").unwrap();
    }
}

#[test]
fn manifest_and_tags_keep_byte_predicates_nested_order_existing_files_and_counts() {
    let directory = tempfile::tempdir().unwrap();
    crate::service_plan::paths::mark_test_project_root(directory.path());
    let mut plan = fixtures::fixture(directory.path(), 0, 1);
    decorate_manifest_tags(&mut plan);
    existing_sidecars(&plan);
    let expected =
        body_files::file_manifest(&body_files::FileManifestInput::from_plan(&plan).unwrap());
    let database = fixtures::database(directory.path(), &plan);
    let store = PlanStore::open(database.path()).unwrap();
    let manifest = store.file_manifest(&plan.id).unwrap();
    assert_eq!(manifest, expected);
    assert!(manifest
        .entries
        .iter()
        .any(|entry| entry.kind == "milestone" && entry.title.contains("unicode-wide")));
    assert!(manifest.entries.iter().any(|entry| entry.kind == "element"
        && entry.title.contains("existing-custom")
        && entry.exists));
    assert!(!manifest
        .entries
        .iter()
        .any(|entry| entry.title.contains("first-short")));
    assert!(manifest
        .entries
        .iter()
        .any(|entry| entry.kind == "stride_log"
            && entry.path.ends_with("tasks/fixture-0002/strides.jsonl")
            && !entry.exists));
    assert!(manifest
        .entries
        .iter()
        .any(|entry| entry.kind == "handoff" && entry.title.contains(&u64::MAX.to_string())));
    let tags = store.tag_report(&plan.id).unwrap();
    assert_eq!(tags, tag_model::tag_report(&plan));
    assert_eq!(tags.facets[0].values[0].sections, 2);
    assert_eq!(tags.facets[0].values[0].tasks, 1);
    assert_eq!(tags.facets[0].values[1].tasks, 0);
    assert_eq!(
        tags.topics
            .iter()
            .find(|topic| topic.tag == "topic")
            .unwrap()
            .sections,
        2
    );
    assert_eq!(
        tags.duplicate_warnings,
        vec![tag_model::TagDuplicateWarning {
            a: "a-b".into(),
            b: "ab".into()
        }]
    );
    assert!(matches!(
        store.file_manifest("missing"),
        Err(PlanError::NotFound)
    ));
    assert!(matches!(
        store.tag_report("missing"),
        Err(PlanError::NotFound)
    ));
    database.connection.lock().execute("UPDATE plan_task_milestones SET row_json=json_set(row_json,'$.kind',42) WHERE plan_id=?1 AND json_extract(row_json,'$.id')='unicode-wide'",[&plan.id]).unwrap();
    assert!(matches!(
        store.file_manifest(&plan.id),
        Err(PlanError::Quarantined { .. })
    ));
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.tags',json('[1]')) WHERE plan_id=?1 AND task_id=?2",params![plan.id,plan.tasks[0].id]).unwrap();
    assert!(store.tag_report(&plan.id).is_err());
    assert!(store.plan_loads_for_test().is_empty());
}

#[test]
fn manifest_and_tags_ignore_body_growth_attempts_strides_rounds_and_unselected_milestones() {
    let mut prior = None;
    for (strides, attempts, extra, history, rounds) in [
        (0, 1, 0, 0, 0),
        (2048, 1, 0, 0, 0),
        (0, 2048, 0, 0, 0),
        (0, 1, 32768, 0, 0),
        (0, 1, 0, 2048, 0),
        (0, 1, 0, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        crate::service_plan::paths::mark_test_project_root(directory.path());
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        decorate_manifest_tags(&mut plan);
        plan.tasks[0].body.push_str(&"x".repeat(extra));
        plan.sections[0].body.push_str(&"x".repeat(extra));
        plan.goal
            .as_mut()
            .unwrap()
            .vision
            .push_str(&"x".repeat(extra));
        plan.elements[1].content["body"] = json!("x".repeat(extra));
        for number in 0..history {
            plan.tasks[0].milestones.push(serde_json::from_value(json!({"id":format!("unselected-{number}"),"kind":"note","text":"short old note","from":{},"ts":1})).unwrap());
        }
        plan.strides.extend(fixtures::round_strides(
            (0..rounds)
                .map(|number| crate::plan::CuratorRound {
                    number,
                    ended_at: number,
                    ..Default::default()
                })
                .collect(),
        ));
        existing_sidecars(&plan);
        let expected =
            body_files::file_manifest(&body_files::FileManifestInput::from_plan(&plan).unwrap());
        let expected_tags = tag_model::tag_report(&plan);
        let database = fixtures::database(directory.path(), &plan);
        let store = PlanStore::open(database.path()).unwrap();
        fixtures::audit(&database);
        crate::service_plan::store_sync::take_decoded_row_bytes();
        database.load_plan(&plan.id).unwrap().unwrap();
        let baseline = crate::service_plan::store_sync::take_decoded_row_bytes();
        take_metrics();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let manifest = store.file_manifest(&plan.id).unwrap();
        let tags = store.tag_report(&plan.id).unwrap();
        assert_eq!(manifest, expected);
        assert_eq!(tags, expected_tags);
        assert!(store.plan_loads_for_test().is_empty());
        let response_bytes = serde_json::to_vec(&json!([manifest, tags])).unwrap().len();
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        assert_eq!(metrics.queries, 9);
        assert!(metrics.materialized_bytes < 262144);
        assert!(decoded < 262144);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=manifest_tag tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={extra} unselected_target_history={history} rounds={rounds} baseline_decoded_json_bytes={baseline} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response_bytes} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response_bytes);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}

#[test]
fn manifest_reads_storage_bucket_and_task_names_from_one_commit() {
    let directory = tempfile::tempdir().unwrap();
    crate::service_plan::paths::mark_test_project_root(directory.path());
    let mut plan = fixtures::fixture(directory.path(), 0, 1);
    decorate_manifest_tags(&mut plan);
    plan.tasks[0].name = "A".into();
    let database = fixtures::database(directory.path(), &plan);
    let store = PlanStore::open(database.path()).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let writer_barrier = barrier.clone();
    let path = database.path();
    let plan_id = plan.id.clone();
    let task_id = plan.tasks[0].id.clone();
    let writer = std::thread::spawn(move || {
        let mut connection = rusqlite::Connection::open(path).unwrap();
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        for index in 0..16 {
            let (status, name) = if index % 2 == 0 {
                ("archived", "B")
            } else {
                ("active", "A")
            };
            let tx = connection.transaction().unwrap();
            tx.execute("UPDATE plans SET status=?2,header_json=json_set(header_json,'$.status',?2) WHERE id=?1",params![plan_id,status]).unwrap();
            tx.execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.name',?3) WHERE plan_id=?1 AND task_id=?2",params![plan_id,task_id,name]).unwrap();
            writer_barrier.wait();
            tx.commit().unwrap();
            writer_barrier.wait();
        }
    });
    let mut coherent = true;
    for _ in 0..16 {
        barrier.wait();
        let manifest = store.file_manifest(&plan.id);
        barrier.wait();
        coherent &= manifest.is_ok_and(|manifest| {
            manifest
                .entries
                .iter()
                .find(|entry| entry.kind == "task")
                .is_some_and(|task| {
                    if task.title.ends_with("(A)") {
                        manifest.index_path.contains("/active/")
                    } else if task.title.ends_with("(B)") {
                        manifest.index_path.contains("/archive/")
                    } else {
                        false
                    }
                })
        });
    }
    writer.join().unwrap();
    assert!(coherent);
}
