use super::*;

pub(in crate::service_plan::store_db) const SEED: &str = "sql-hot-1162-v1";
pub(in crate::service_plan::store_db) const TASKS: usize = 1162;
pub(in crate::service_plan::store_db) const CONTENT_BYTES: usize = 33_344_717;

pub(super) fn milestone(id: &str) -> Milestone {
    Milestone {
        id: id.into(),
        from: AgentRef::from_tab("worker"),
        kind: "decision".into(),
        text: "bounded append".into(),
        ts: 20,
        links: vec![],
        loop_id: None,
        iteration: None,
        canvas: Default::default(),
        attachments: vec![],
    }
}

/// Curator round history lives in `curator_round` strides, never in the header.
pub(in crate::service_plan::store_db) fn round_strides(
    rounds: Vec<crate::plan::CuratorRound>,
) -> Vec<Milestone> {
    rounds
        .into_iter()
        .enumerate()
        .map(|(index, round)| Milestone {
            id: format!("round-{index}"),
            from: Default::default(),
            kind: "curator_round".into(),
            text: serde_json::to_string(&round).unwrap(),
            ts: round.ended_at,
            links: vec![],
            loop_id: None,
            iteration: None,
            canvas: Default::default(),
            attachments: vec![],
        })
        .collect()
}

pub(in crate::service_plan::store_db) fn fixture(
    root: &std::path::Path,
    strides: usize,
    attempts: usize,
) -> Plan {
    let mut plan = Plan::new(
        SEED.into(),
        root.display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.id = SEED.into();
    plan.created_at = 1;
    plan.updated_at = 1;
    plan.tasks = (0..TASKS)
        .map(|i| {
            let mut task = load_audit::task(&format!("task_{i:04}"), TaskStatus::Pending, None);
            task.handle = Some(format!("fixture-{i:04}"));
            task.updated_at = 1;
            task
        })
        .collect();
    let mut current = load_audit::attempt("current", "worker");
    current.session_id = Some("session".into());
    current.session_binding_nonce = Some("nonce".into());
    plan.tasks[0].execution_attempts.push(current);
    let empty_bytes = serde_json::to_vec(&plan).unwrap().len();
    let fill = CONTENT_BYTES - empty_bytes;
    for (i, task) in plan.tasks.iter_mut().enumerate() {
        task.body = "x".repeat(fill / TASKS + usize::from(i < fill % TASKS));
    }
    assert_eq!(serde_json::to_vec(&plan).unwrap().len(), CONTENT_BYTES);
    plan.strides = (0..strides)
        .map(|i| milestone(&format!("history-{i}")))
        .collect();
    plan.tasks[1].execution_attempts = (0..attempts)
        .map(|i| {
            let mut attempt = load_audit::attempt(&format!("old-{i}"), "unrelated");
            attempt.closed_at = Some(2);
            attempt
        })
        .collect();
    plan
}

pub(in crate::service_plan::store_db) fn database(
    root: &std::path::Path,
    plan: &Plan,
) -> PlanDatabase {
    crate::service_plan::paths::mark_test_project_root(root);
    let database = PlanDatabase::open(root.join("plans.db")).unwrap();
    database.persist_plan_rows(plan, None).unwrap();
    let mut spec = crate::plan::Spec::new("owned spec".into(), plan.project_path.clone());
    spec.plan_id = Some(plan.id.clone());
    database.persist_spec(&spec).unwrap();
    database
}

pub(super) fn role(value: &str) -> HotChange {
    HotChange::Task {
        selector: "task_0000".into(),
        change: TaskMetadata::RoleGuidance(Some(Some(value.into()))),
    }
}

pub(in crate::service_plan::store_db) fn audit(database: &PlanDatabase) {
    let connection = database.connection.lock();
    connection
        .execute_batch("CREATE TABLE write_audit(table_name TEXT);")
        .unwrap();
    for table in [
        "plans",
        "plan_tasks",
        "plan_sections",
        "plan_elements",
        "plan_task_milestones",
        "plan_task_attempts",
        "plan_strides",
        "specs",
    ] {
        for action in ["INSERT", "UPDATE", "DELETE"] {
            connection.execute_batch(&format!("CREATE TRIGGER audit_{table}_{action} AFTER {action} ON {table} BEGIN INSERT INTO write_audit VALUES ('{table}'); END;")).unwrap();
        }
    }
}

pub(in crate::service_plan::store_db) fn audit_counts(
    database: &PlanDatabase,
) -> Vec<(String, usize)> {
    let connection = database.connection.lock();
    let mut statement = connection
        .prepare(
            "SELECT table_name, count(*) FROM write_audit GROUP BY table_name ORDER BY table_name",
        )
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

pub(super) fn database_for_baseline(root: &std::path::Path, plan: &Plan) -> PlanDatabase {
    database(root, plan)
}

pub(in crate::service_plan::store_db) fn baseline_operation(
    database: &PlanDatabase,
    id: &str,
    kind: &str,
    mutate: impl FnOnce(&mut Plan),
) {
    crate::service_plan::store_sync::take_decoded_row_bytes();
    store_rows::take_serialized_row_bytes();
    let started = Instant::now();
    let mut plan = database.load_plan(id).unwrap().unwrap();
    mutate(&mut plan);
    plan.updated_at += 1;
    let writes = database.persist_plan_rows(&plan, None).unwrap();
    let elapsed = started.elapsed();
    let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
    let serialized = store_rows::take_serialized_row_bytes();
    println!("baseline=load_plan+persist_plan_rows operation={kind} strides={} unrelated_attempts={} decoded_row_bytes={decoded} serialized_row_bytes={serialized} elapsed_us={} writes={writes:?}", plan.strides.len(), plan.tasks[1].execution_attempts.len(), elapsed.as_micros());
    assert!(serialized > 31 * 1024 * 1024);
}

pub(super) fn assert_and_clear_writes(database: &PlanDatabase, receipt: &CommittedRows) {
    let mut expected: Vec<(String, usize)> = [
        ("plan_tasks", receipt.task_rows),
        ("plan_sections", receipt.section_rows),
        ("plan_elements", receipt.element_rows),
        ("specs", receipt.spec_rows),
        ("plans", receipt.header_rows),
        ("plan_task_attempts", receipt.attempts),
        ("plan_task_milestones", receipt.milestones),
        ("plan_strides", receipt.strides),
    ]
    .into_iter()
    .filter(|(_, count)| *count > 0)
    .map(|(table, count)| (table.into(), count))
    .collect();
    expected.sort();
    assert_eq!(audit_counts(database), expected);
    database
        .connection
        .lock()
        .execute("DELETE FROM write_audit", [])
        .unwrap();
}

// Test observers are deliberately outside the admitted durable schema.
pub(in crate::service_plan::store_db) fn remove_observers(database: &PlanDatabase) {
    let connection = database.connection.lock();
    let names = {
        let mut statement = connection
            .prepare("SELECT name FROM sqlite_schema WHERE type='trigger'")
            .unwrap();
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap();
        rows.map(Result::unwrap).collect::<Vec<_>>()
    };
    for name in names {
        connection
            .execute_batch(&format!("DROP TRIGGER \"{}\";", name.replace('"', "\"\"")))
            .unwrap();
    }
    connection
        .execute_batch("DROP TABLE IF EXISTS write_audit;")
        .unwrap();
}
