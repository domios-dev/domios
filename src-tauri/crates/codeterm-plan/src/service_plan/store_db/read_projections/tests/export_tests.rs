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

#[test]
fn bounded_plan_export_caps_tasks_history_and_strides_while_preserving_small_plans() {
    let directory = tempfile::tempdir().unwrap();
    let mut small = Plan::new(
        "Small".into(),
        directory.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    small.id = "pl_small".into();
    let mut task =
        crate::service_plan::load_audit::task("t1", TaskStatus::InProgress, Some("worker"));
    task.milestones = (0..3)
        .map(|i| {
            serde_json::from_value(
                json!({"id":format!("m{i}"),"from":{},"kind":"note","text":format!("m {i}"),"ts":i}),
            )
            .unwrap()
        })
        .collect();
    task.execution_attempts = vec![crate::service_plan::load_audit::attempt("a0", "worker")];
    small.tasks = vec![task];
    let database = fixtures::database(directory.path(), &small);
    let store = PlanStore::open(database.path()).unwrap();
    let limits = crate::service_plan::BoundedPlanExportLimits::default();
    let unbounded = store.export_plan_document(&small.id).unwrap();
    let bounded_small = store
        .export_plan_document_bounded(&small.id, limits)
        .unwrap();
    assert_eq!(
        bounded_small, unbounded,
        "plans within bounds match full export verbatim without _bounded metadata"
    );

    let mut large = Plan::new(
        "Large".into(),
        directory.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    large.id = "pl_large".into();
    large.tasks = (0..12)
        .map(|i| {
            let status = if i == 11 {
                TaskStatus::InProgress
            } else {
                TaskStatus::Done
            };
            let mut t =
                crate::service_plan::load_audit::task(&format!("t{i}"), status, Some("worker"));
            t.created_at = 100 + i as u64;
            t.milestones = (0..8)
                .map(|m| {
                    serde_json::from_value(json!({
                        "id": format!("m-{i}-{m}"),
                        "from": {},
                        "kind": "note",
                        "text": format!("milestone {m}"),
                        "ts": m
                    }))
                    .unwrap()
                })
                .collect();
            t.execution_attempts = (0..4)
                .map(|a| crate::service_plan::load_audit::attempt(&format!("a-{i}-{a}"), "worker"))
                .collect();
            t
        })
        .collect();
    large.strides = (0..15)
        .map(|s| {
            serde_json::from_value(json!({
                "id": format!("s{s}"),
                "from": {},
                "kind": "decision",
                "text": format!("stride {s}"),
                "ts": s
            }))
            .unwrap()
        })
        .collect();
    database.persist_plan(&large, None).unwrap();

    let tight = crate::service_plan::BoundedPlanExportLimits {
        max_tasks: 4,
        max_task_milestones: 2,
        max_task_attempts: 1,
        max_strides: 3,
    };
    let bounded = store
        .export_plan_document_bounded(&large.id, tight)
        .unwrap();
    let tasks = bounded["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 4);
    assert!(
        tasks.iter().any(|t| t["id"] == "t11"),
        "open tasks are prioritized into the bounded window"
    );
    for task in tasks {
        let milestones = task["milestones"].as_array().unwrap();
        assert_eq!(milestones.len(), 2);
        assert_eq!(milestones[0]["text"], "milestone 6");
        assert_eq!(milestones[1]["text"], "milestone 7");
        let attempts = task["execution_attempts"].as_array().unwrap();
        assert_eq!(attempts.len(), 1);
    }
    let strides = bounded["strides"].as_array().unwrap();
    assert_eq!(strides.len(), 3);
    assert_eq!(strides[0]["text"], "stride 12");
    assert_eq!(strides[2]["text"], "stride 14");
    assert_eq!(bounded["_bounded"]["truncated"], true);
    assert_eq!(bounded["_bounded"]["tasks_total"], 12);
    assert_eq!(bounded["_bounded"]["tasks_returned"], 4);
    assert_eq!(bounded["_bounded"]["strides_total"], 15);
    assert_eq!(bounded["_bounded"]["strides_returned"], 3);
    assert_eq!(bounded["_bounded"]["task_history_truncated"], true);
}
