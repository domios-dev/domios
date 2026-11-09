use super::*;
use crate::plan::{PlanMode, PlannerRef, TaskStatus};

#[test]
fn chat_attempt_timestamps_preserve_none_missing_values_and_latest_ties() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Chat".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    let cases = [
        ("task_null", vec![("null_first", None), ("null_last", None)]),
        (
            "task_tie",
            vec![
                ("tie_first", Some(20)),
                ("tie_last", Some(20)),
                ("undated", None),
            ],
        ),
        ("task_missing", vec![("missing", None)]),
    ];
    for (id, attempts) in cases {
        let mut task = crate::service_plan::load_audit::task(id, TaskStatus::Pending, None);
        task.execution_attempts = attempts
            .into_iter()
            .map(|(id, assigned_at)| {
                let mut attempt = crate::service_plan::load_audit::attempt(id, "worker");
                attempt.assigned_at = assigned_at;
                attempt
            })
            .collect();
        plan.tasks.push(task);
    }
    let database = fixtures::database(directory.path(), &plan);
    database.connection.lock().execute(
        "UPDATE plan_task_attempts SET row_json=json_remove(row_json,'$.assigned_at') WHERE attempt_id='missing'",[],
    ).unwrap();
    fixtures::audit(&database);
    let rows = database
        .chat_projection(None, None, None, Some(&plan.id), None)
        .unwrap();
    for task in &plan.tasks {
        let projected = rows[0]["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == task.id)
            .unwrap();
        let expected = task
            .execution_attempts
            .iter()
            .max_by_key(|attempt| attempt.assigned_at)
            .unwrap();
        assert_eq!(projected["execution_attempts"][0]["id"], expected.id);
        assert_eq!(
            projected["execution_attempts"][0]["assigned_at"],
            json!(expected.assigned_at)
        );
    }
    assert!(fixtures::audit_counts(&database).is_empty());
    database.connection.lock().execute(
        "UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.assigned_at','invalid') WHERE attempt_id='tie_last'",[],
    ).unwrap();
    assert!(matches!(
        database.chat_projection(None, None, None, Some(&plan.id), None),
        Err(PlanError::Quarantined { .. })
    ));
}

#[test]
fn chat_header_membership_preserves_all_four_binding_paths_and_filters() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Chat".into(),
        directory.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.mode = PlanMode::Execution;
    plan.team_id = Some("team".into());
    plan.planner = Some(PlannerRef::Tab {
        tab_id: "planner".into(),
        session_id: None,
        title: None,
    });
    let mut task =
        crate::service_plan::load_audit::task("task_one", TaskStatus::InProgress, Some("assigned"));
    task.lane.tab_id = Some("lane".into());
    task.key = Some("QA-1".into());
    plan.tasks = vec![task];
    let database = fixtures::database(directory.path(), &plan);
    for tab in ["owner", "planner", "assigned", "lane"] {
        let rows = database
            .chat_projection(None, None, None, None, Some(tab))
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["id"], plan.id);
        assert_eq!(
            rows[0]["tasks"].as_array().unwrap().len(),
            usize::from(matches!(tab, "assigned" | "lane"))
        );
        if matches!(tab, "assigned" | "lane") {
            assert_eq!(rows[0]["tasks"][0]["key"], "QA-1");
            assert_eq!(rows[0]["tasks"][0]["body"], "");
        }
    }
    assert!(database
        .chat_projection(None, Some("other"), None, None, Some("lane"))
        .unwrap()
        .is_empty());
    assert!(database
        .chat_projection(Some("other"), None, None, None, Some("lane"))
        .unwrap()
        .is_empty());
    assert!(database
        .chat_projection(None, None, None, Some("other"), Some("lane"))
        .unwrap()
        .is_empty());
    plan.owner.tab_id = Some("same".into());
    plan.planner = Some(PlannerRef::Tab {
        tab_id: "same".into(),
        session_id: None,
        title: None,
    });
    plan.tasks[0].assigned_tab_id = Some("same".into());
    plan.tasks[0].lane.tab_id = Some("same".into());
    database.persist_plan(&plan, None).unwrap();
    assert_eq!(
        database
            .chat_projection(None, None, None, None, Some("same"))
            .unwrap()
            .len(),
        1
    );
    plan.status = crate::plan::PlanStatus::Archived;
    database.persist_plan(&plan, None).unwrap();
    assert_eq!(
        database
            .chat_projection(None, None, None, None, Some("same"))
            .unwrap()
            .len(),
        1
    );
    assert!(database
        .chat_projection(None, None, Some(""), None, Some("same"))
        .unwrap()
        .is_empty());
}

#[test]
fn unrelated_chat_tab_uses_only_indexed_membership_with_thousands_of_tasks() {
    let mut prior = None;
    for (extra_tasks, other_plans, strides, attempts, extra_body) in [
        (0, 0, 0, 1, 0),
        (4000, 0, 0, 1, 0),
        (4000, 131, 2048, 2048, 32768),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.tasks[0].body.push_str(&"x".repeat(extra_body));
        for index in 0..extra_tasks {
            let mut task = crate::service_plan::load_audit::task(
                &format!("noise-{index:04}"),
                TaskStatus::InProgress,
                Some("other-worker"),
            );
            task.lane.tab_id = Some("other-lane".into());
            plan.tasks.push(task);
        }
        let database = fixtures::database(directory.path(), &plan);
        for index in 0..other_plans {
            let mut other = Plan::new(
                format!("Other {index}"),
                plan.project_path.clone(),
                OrchestratorRef::from_tab(&format!("owner-{index}")),
            );
            other.planner = Some(PlannerRef::Tab {
                tab_id: format!("planner-{index}"),
                session_id: None,
                title: None,
            });
            database.persist_plan(&other, None).unwrap();
        }
        let details: Vec<String> = {
            let connection = database.connection.lock();
            let sql = super::super::super::chat_read_queries::header_sql(true);
            let mut statement = connection
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .unwrap();
            let rows = statement
                .query_map(
                    params![
                        Option::<String>::None,
                        Option::<String>::None,
                        Option::<String>::None,
                        Option::<String>::None,
                        "unbound"
                    ],
                    |row| row.get(3),
                )
                .unwrap();
            rows.collect::<rusqlite::Result<_>>().unwrap()
        };
        for index in [
            "plans_owner_tab_id",
            "plans_planner_tab_id",
            "plan_tasks_assigned_tab_id",
            "plan_tasks_lane_tab_id",
        ] {
            assert!(
                details
                    .iter()
                    .any(|detail| detail.starts_with("SEARCH ") && detail.contains(index)),
                "{details:?}"
            );
        }
        assert!(
            details
                .iter()
                .any(|detail| detail.starts_with("SEARCH p ") && detail.contains("(id=?)")),
            "{details:?}"
        );
        assert!(
            !details.iter().any(|detail| detail.starts_with("SCAN p ")
                || detail == "SCAN p"
                || detail.starts_with("SCAN plans")
                || detail.starts_with("SCAN plan_tasks")),
            "{details:?}"
        );
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let start = std::time::Instant::now();
        let rows = database
            .chat_projection(None, None, None, None, Some("unbound"))
            .unwrap();
        let elapsed = start.elapsed().as_micros();
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        let response_bytes = serde_json::to_vec(&rows).unwrap().len();
        assert!(rows.is_empty());
        assert_eq!(metrics.queries, 1);
        assert_eq!(metrics.materialized_bytes, 0);
        assert_eq!(decoded, 0);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=chat_header_unbound tasks={} plans={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={extra_body} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response_bytes} changed_rows=0 elapsed_us={elapsed}",fixtures::SEED,plan.tasks.len(),other_plans+1,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response_bytes);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}

#[test]
fn chat_membership_and_selected_rows_share_a_snapshot_and_fail_loudly() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Coherent chat".into(),
        directory.path().display().to_string(),
        OrchestratorRef::from_tab("worker"),
    );
    plan.tasks = vec![crate::service_plan::load_audit::task(
        "task_one",
        TaskStatus::InProgress,
        Some("other"),
    )];
    let database = fixtures::database(directory.path(), &plan);
    let path = database.path();
    let id = plan.id.clone();
    let barrier = Arc::new(Barrier::new(2));
    let writer_barrier = barrier.clone();
    let writer = std::thread::spawn(move || {
        let mut connection = rusqlite::Connection::open(path).unwrap();
        connection
            .busy_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        for index in 0..16 {
            let (owner, assigned) = if index % 2 == 0 {
                ("other", "worker")
            } else {
                ("worker", "other")
            };
            let tx = connection.transaction().unwrap();
            tx.execute("UPDATE plans SET header_json=json_set(header_json,'$.owner.tab_id',?2) WHERE id=?1",params![id,owner]).unwrap();
            tx.execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.assigned_tab_id',?2) WHERE plan_id=?1",params![id,assigned]).unwrap();
            writer_barrier.wait();
            tx.commit().unwrap();
            writer_barrier.wait();
        }
    });
    for _ in 0..16 {
        barrier.wait();
        let rows = database
            .chat_projection(None, None, None, None, Some("worker"))
            .unwrap();
        assert_eq!(rows.len(), 1);
        let assigned = !rows[0]["tasks"].as_array().unwrap().is_empty();
        assert_eq!(rows[0]["owner"]["tab_id"] == "other", assigned);
        barrier.wait();
    }
    writer.join().unwrap();
    database
        .connection
        .lock()
        .execute(
            "UPDATE plans SET header_json=json_set(header_json,'$.owner.tab_id',17) WHERE id=?1",
            [&plan.id],
        )
        .unwrap();
    assert!(matches!(
        database.chat_projection(None, None, None, None, None),
        Err(PlanError::Quarantined { .. })
    ));
}
