use super::*;
use crate::plan::{CuratorRound, Goal, PlanMode, PlannerRef, TaskStatus};

#[test]
fn full_export_preserves_record_shape_history_order_and_requested_plan_order() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Full α".into(),
        directory.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.id = "pl_z".into();
    plan.mode = PlanMode::Execution;
    plan.planner = Some(PlannerRef::Waived {
        note: "solo".into(),
    });
    plan.goal = Some(Goal {
        vector: "ship".into(),
        vision: "complete document".into(),
        constraints: vec!["keep".into()],
        postulates: vec![],
    });
    plan.sections = sections();
    plan.elements =
        serde_json::from_value(json!([{"id":"note","kind":"note","content":"full prose"}]))
            .unwrap();
    let mut task =
        crate::service_plan::load_audit::task("task_full", TaskStatus::InProgress, Some("worker"));
    task.body = "complete body α".into();
    task.interruption.owner_epoch = 7;
    task.milestones=(0..75).map(|index|serde_json::from_value(json!({"id":format!("m{index}"),"from":{},"kind":"note","text":format!("full {index}"),"ts":index})).unwrap()).collect();
    task.execution_attempts = (0..20)
        .map(|index| crate::service_plan::load_audit::attempt(&format!("a{index}"), "worker"))
        .collect();
    plan.tasks = vec![task];
    plan.strides = plan.tasks[0].milestones.clone();
    plan.strides
        .extend(fixtures::round_strides(vec![CuratorRound {
            number: 1,
            summary: "retained complete round".into(),
            ..Default::default()
        }]));
    let database = fixtures::database(directory.path(), &plan);
    let mut second = plan.clone();
    second.id = "pl_a".into();
    second.title = "Second".into();
    database.persist_plan(&second, None).unwrap();
    let store = PlanStore::open(database.path()).unwrap();
    fixtures::audit(&database);
    let exported = store.export_plan_document(&plan.id).unwrap();
    assert_eq!(exported, serde_json::to_value(&plan).unwrap());
    assert_eq!(
        exported["tasks"][0]["milestones"].as_array().unwrap().len(),
        75
    );
    assert_eq!(
        exported["tasks"][0]["execution_attempts"]
            .as_array()
            .unwrap()
            .len(),
        20
    );
    assert!(exported.get("_revision").is_none());
    let ordered = store
        .export_plan_documents(&[second.id.clone(), plan.id.clone(), second.id.clone()])
        .unwrap();
    assert_eq!(
        ordered,
        vec![
            serde_json::to_value(&second).unwrap(),
            exported,
            serde_json::to_value(&second).unwrap()
        ]
    );
    let all = store.export_all_plan_documents().unwrap();
    assert_eq!(
        all.iter()
            .map(|value| value["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["pl_a", "pl_z"]
    );
    let filtered = store
        .export_plan_list(Some(&plan.project_path), None, false, Some("owner"))
        .unwrap();
    assert_eq!(
        filtered
            .iter()
            .map(|value| value["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["pl_z", "pl_a"]
    );
    assert!(matches!(
        store.export_plan_documents(&[plan.id.clone(), "missing".into()]),
        Err(PlanError::NotFound)
    ));
    assert!(store.export_plan_documents(&[]).unwrap().is_empty());
    assert!(store.plan_loads_for_test().is_empty());
    assert!(fixtures::audit_counts(&database).is_empty());
}

struct OutputBytes(usize);
impl std::io::Write for OutputBytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn full_export_materialization_is_proportional_to_the_requested_document() {
    for (strides, attempts, extra_body, rounds) in [
        (0, 1, 0, 0),
        (2048, 1, 0, 0),
        (0, 2048, 0, 0),
        (0, 1, 32768, 0),
        (0, 1, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.tasks[1].body.push_str(&"x".repeat(extra_body));
        plan.strides.extend(fixtures::round_strides(
            (0..rounds)
                .map(|number| CuratorRound {
                    number,
                    ..Default::default()
                })
                .collect(),
        ));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let document = database.export_plan_document(&plan.id).unwrap();
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        let mut output = OutputBytes(0);
        serde_json::to_writer(&mut output, &document).unwrap();
        let response_bytes = output.0;
        assert_eq!(metrics.queries, 8);
        assert_eq!(serialized, 0);
        assert!(metrics.materialized_bytes <= response_bytes + 128 * 1024);
        assert!(decoded <= response_bytes + 128 * 1024);
        assert_eq!(document["tasks"].as_array().unwrap().len(), fixtures::TASKS);
        assert_eq!(document["tasks"][1]["body"], plan.tasks[1].body);
        let exported = document["strides"].as_array().unwrap();
        assert_eq!(exported.len(), strides + rounds as usize);
        assert_eq!(
            exported
                .iter()
                .filter(|stride| stride["kind"] == "curator_round")
                .count(),
            rounds as usize
        );
        assert!(document["control_loop"].get("rounds").is_none());
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=full_document_export tasks={} base_bytes={} requested_strides={strides} requested_attempts={attempts} requested_extra_body={extra_body} requested_rounds={rounds} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response_bytes} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
    }
}

#[test]
fn full_export_multi_plan_and_component_reads_share_one_committed_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "A".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.tasks = vec![crate::service_plan::load_audit::task(
        "one",
        TaskStatus::Pending,
        None,
    )];
    plan.tasks[0].body = "A".into();
    let database = fixtures::database(directory.path(), &plan);
    let mut second = plan.clone();
    second.id = "pl_second".into();
    second.tasks.clear();
    database.persist_plan(&second, None).unwrap();
    let path = database.path();
    let barrier = Arc::new(Barrier::new(2));
    let writer_barrier = barrier.clone();
    let writer = std::thread::spawn(move || {
        let mut connection = rusqlite::Connection::open(path).unwrap();
        connection
            .busy_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        for index in 0..16 {
            let value = if index % 2 == 0 { "B" } else { "A" };
            let tx = connection.transaction().unwrap();
            tx.execute(
                "UPDATE plans SET title=?1,header_json=json_set(header_json,'$.title',?1)",
                [value],
            )
            .unwrap();
            tx.execute(
                "UPDATE plan_tasks SET row_json=json_set(row_json,'$.body',?1)",
                [value],
            )
            .unwrap();
            writer_barrier.wait();
            tx.commit().unwrap();
            writer_barrier.wait();
        }
    });
    for _ in 0..16 {
        barrier.wait();
        let values = database
            .export_plan_documents(&[plan.id.clone(), second.id.clone()])
            .unwrap();
        assert_eq!(values[0]["title"], values[0]["tasks"][0]["body"]);
        assert_eq!(values[0]["title"], values[1]["title"]);
        barrier.wait();
    }
    writer.join().unwrap();
}

#[test]
fn full_export_rejects_malformed_selected_records_and_embedded_whole_plan_rows() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Selected".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.tasks = vec![crate::service_plan::load_audit::task(
        "one",
        TaskStatus::Pending,
        None,
    )];
    let database = fixtures::database(directory.path(), &plan);
    let mut other = plan.clone();
    other.id = "pl_other".into();
    database.persist_plan(&other, None).unwrap();
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.status','invalid') WHERE plan_id=?1",[&other.id]).unwrap();
    assert!(database.export_plan_document(&plan.id).is_ok());
    assert!(matches!(
        database.export_plan_documents(&[plan.id.clone(), other.id]),
        Err(PlanError::Quarantined { .. })
    ));
    database
        .connection
        .lock()
        .execute(
            "UPDATE plans SET header_json=json_set(header_json,'$.tasks',json('[]')) WHERE id=?1",
            [&plan.id],
        )
        .unwrap();
    assert!(matches!(
        database.export_plan_document(&plan.id),
        Err(PlanError::Quarantined { .. })
    ));
}

#[test]
fn full_export_filters_and_document_rows_share_a_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let mut first = Plan::new("First".into(), "wanted".into(), OrchestratorRef::default());
    first.id = "pl_first".into();
    let database = fixtures::database(directory.path(), &first);
    let mut second = first.clone();
    second.id = "pl_second".into();
    second.project_path = "other".into();
    database.persist_plan(&second, None).unwrap();
    let id = first.id.clone();
    let path = database.path();
    let barrier = Arc::new(Barrier::new(2));
    let writer_barrier = barrier.clone();
    let writer = std::thread::spawn(move || {
        let mut connection = rusqlite::Connection::open(path).unwrap();
        connection
            .busy_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        for index in 0..16 {
            let (first, second) = if index % 2 == 0 {
                ("other", "wanted")
            } else {
                ("wanted", "other")
            };
            let tx = connection.transaction().unwrap();
            tx.execute("UPDATE plans SET project_path=CASE id WHEN ?1 THEN ?2 ELSE ?3 END,
                header_json=json_set(header_json,'$.project_path',CASE id WHEN ?1 THEN ?2 ELSE ?3 END)",params![id,first,second]).unwrap();
            writer_barrier.wait();
            tx.commit().unwrap();
            writer_barrier.wait();
        }
    });
    for _ in 0..16 {
        barrier.wait();
        let documents = database
            .export_plan_list(Some("wanted"), None, false, None)
            .unwrap();
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0]["project_path"], "wanted");
        barrier.wait();
    }
    writer.join().unwrap();
}
