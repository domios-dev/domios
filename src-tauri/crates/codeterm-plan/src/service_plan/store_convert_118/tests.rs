use super::*;
use crate::plan::{Milestone, OrchestratorRef, PlanningState, Task, TaskExecutionAttempt};
use crate::service_plan::store_db::PlanDatabase;
use crate::service_plan::Spec;
use serde_json::json;
use std::collections::BTreeMap;

const SNAPSHOT_ENV: &str = "DOMIOS_PLAN_SNAPSHOT";

// The 1.17 plans.db schema, kept only to build legacy fixtures.
const LEGACY_SCHEMA: &str = r#"
    CREATE TABLE plans (
        id TEXT PRIMARY KEY,
        title TEXT NOT NULL,
        status TEXT NOT NULL,
        team_id TEXT,
        project_path TEXT NOT NULL,
        updated_at INTEGER NOT NULL,
        document_json TEXT NOT NULL
    );
    CREATE UNIQUE INDEX one_active_plan_per_team
        ON plans(team_id)
        WHERE team_id IS NOT NULL AND status = 'active';
    CREATE INDEX plans_status_updated ON plans(status, updated_at DESC);
    CREATE TABLE specs (
        id TEXT PRIMARY KEY,
        plan_id TEXT,
        project_path TEXT NOT NULL,
        updated_at INTEGER NOT NULL,
        document_json TEXT NOT NULL,
        FOREIGN KEY(plan_id) REFERENCES plans(id) ON DELETE CASCADE
    );
    CREATE TABLE plan_operations (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        entity_id TEXT NOT NULL,
        kind TEXT NOT NULL,
        state TEXT NOT NULL CHECK(state IN ('queued','running','committed','failed')),
        submitted_at INTEGER NOT NULL,
        finished_at INTEGER,
        error TEXT
    );
    CREATE INDEX plan_operations_state_id ON plan_operations(state, id);
    CREATE VIRTUAL TABLE plan_fts USING fts5(plan_id UNINDEXED, title, body);
    CREATE TABLE plan_tasks (
        plan_id TEXT NOT NULL,
        task_id TEXT NOT NULL,
        position INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        content_hash INTEGER NOT NULL,
        milestone_count INTEGER NOT NULL,
        milestone_digest INTEGER NOT NULL,
        document_json TEXT NOT NULL,
        PRIMARY KEY (plan_id, task_id),
        FOREIGN KEY (plan_id) REFERENCES plans(id) ON DELETE CASCADE
    );
    CREATE TABLE plan_task_milestones (
        plan_id TEXT NOT NULL,
        task_id TEXT NOT NULL,
        seq INTEGER NOT NULL,
        document_json TEXT NOT NULL,
        PRIMARY KEY (plan_id, task_id, seq),
        FOREIGN KEY (plan_id, task_id) REFERENCES plan_tasks(plan_id, task_id) ON DELETE CASCADE
    );
    CREATE TABLE plan_task_attempts (
        plan_id TEXT NOT NULL,
        task_id TEXT NOT NULL,
        attempt_id TEXT NOT NULL,
        position INTEGER NOT NULL,
        content_hash INTEGER NOT NULL,
        document_json TEXT NOT NULL,
        PRIMARY KEY (plan_id, task_id, position),
        FOREIGN KEY (plan_id, task_id) REFERENCES plan_tasks(plan_id, task_id) ON DELETE CASCADE
    );
"#;

fn milestone(id: &str) -> Milestone {
    Milestone {
        id: id.into(),
        from: OrchestratorRef::from_tab("worker"),
        kind: "note".into(),
        text: format!("text of {id}"),
        ts: 5,
        links: vec![],
        loop_id: None,
        iteration: None,
        canvas: Default::default(),
        attachments: vec![],
    }
}

fn task(id: &str, milestones: usize, attempts: usize) -> Task {
    let mut task: Task = serde_json::from_value(json!({
        "id": id, "name": format!("Task {id}"), "type": "implement", "status": "in_progress",
        "assigned_tab_id": format!("tab-{id}"),
        "lane": {"worktree_path": format!("/lanes/{id}"), "branch": format!("lane/{id}")}
    }))
    .unwrap();
    task.milestones = (0..milestones)
        .map(|seq| milestone(&format!("{id}-m{seq}")))
        .collect();
    task.execution_attempts = (0..attempts)
        .map(|seq| TaskExecutionAttempt {
            id: format!("{id}-a{seq}"),
            executor_tab_id: format!("tab-{seq}"),
            session_id: Some(format!("session-{id}-{seq}")),
            closed_at: (seq == 0 && attempts > 1).then_some(9),
            ..Default::default()
        })
        .collect();
    task
}

fn legacy_plan(id: &str) -> Plan {
    let mut plan = Plan::new(
        "Legacy plan".into(),
        "/repo".into(),
        OrchestratorRef::from_tab("orch"),
    );
    plan.id = id.into();
    plan.updated_at = 5;
    plan.planning_state = PlanningState::Approved;
    plan.sections = serde_json::from_value(json!([
        {"id": "sec_b", "title": "Second", "body": "beta"},
        {"id": "sec_a", "title": "First", "body": "alpha", "subsections": [{"id": "sec_a1", "title": "Nested"}]}
    ]))
    .unwrap();
    plan.elements = serde_json::from_value(json!([
        {"id": "el_1", "kind": "note", "content": {"text": "canvas"}}
    ]))
    .unwrap();
    plan.strides = (0..3).map(|seq| milestone(&format!("st{seq}"))).collect();
    plan.heartbeat.current_focus = Some("conversion".into());
    plan.heartbeat.rules = vec!["keep rows".into()];
    plan.tasks = vec![task("t2", 2, 1), task("t1", 0, 2)];
    plan
}

fn insert_legacy_header(connection: &Connection, plan_id: &str, document: &str) {
    connection
        .execute(
            "INSERT INTO plans(id, title, status, team_id, project_path, updated_at, document_json)
             VALUES (?1, 'Legacy plan', 'active', NULL, '/repo', 5, ?2)",
            params![plan_id, document],
        )
        .unwrap();
}

/// Writes a plan the way 1.17 stored it: the document minus tasks, plus task rows.
fn insert_legacy_plan(connection: &Connection, plan: &Plan) {
    let mut document = serde_json::to_value(plan).unwrap();
    let tasks = document.as_object_mut().unwrap().remove("tasks").unwrap();
    insert_legacy_header(connection, &plan.id, &document.to_string());
    for (position, mut task) in tasks.as_array().unwrap().iter().cloned().enumerate() {
        let fields = task.as_object_mut().unwrap();
        let milestones = fields.remove("milestones").unwrap();
        let attempts = fields.remove("execution_attempts").unwrap();
        let task_id = fields["id"].as_str().unwrap().to_string();
        connection
            .execute(
                "INSERT INTO plan_tasks(plan_id, task_id, position, updated_at, content_hash, milestone_count, milestone_digest, document_json)
                 VALUES (?1, ?2, ?3, 0, 0, 0, 0, ?4)",
                params![plan.id, task_id, position as i64, task.to_string()],
            )
            .unwrap();
        for (seq, milestone) in milestones.as_array().unwrap().iter().enumerate() {
            connection
                .execute(
                    "INSERT INTO plan_task_milestones(plan_id, task_id, seq, document_json) VALUES (?1, ?2, ?3, ?4)",
                    params![plan.id, task_id, seq as i64, milestone.to_string()],
                )
                .unwrap();
        }
        for (position, attempt) in attempts.as_array().unwrap().iter().enumerate() {
            connection
                .execute(
                    "INSERT INTO plan_task_attempts(plan_id, task_id, attempt_id, position, content_hash, document_json)
                     VALUES (?1, ?2, ?3, ?4, 0, ?5)",
                    params![plan.id, task_id, attempt["id"].as_str().unwrap(), position as i64, attempt.to_string()],
                )
                .unwrap();
        }
    }
}

fn legacy_connection(path: &Path) -> Connection {
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
        .unwrap();
    connection
}

fn legacy_database(directory: &Path, plans: &[Plan]) -> PathBuf {
    let path = directory.join("plans.db");
    let connection = legacy_connection(&path);
    connection.execute_batch(LEGACY_SCHEMA).unwrap();
    for plan in plans {
        insert_legacy_plan(&connection, plan);
    }
    path
}

fn legacy_values(path: &Path) -> BTreeMap<String, Result<Value, String>> {
    let connection = Connection::open(path).unwrap();
    read_legacy_plans(&connection)
        .unwrap()
        .into_iter()
        .map(|(id, plan)| (id, plan.map(|plan| serde_json::to_value(plan).unwrap())))
        .collect()
}

fn loaded_values(database: &PlanDatabase) -> BTreeMap<String, Value> {
    database
        .load_snapshots()
        .unwrap()
        .0
        .into_iter()
        .map(|plan| (plan.id.clone(), serde_json::to_value(plan).unwrap()))
        .collect()
}

fn user_version(path: &Path) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap()
}

fn count(path: &Path, sql: &str) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row(sql, [], |row| row.get(0))
        .unwrap()
}

#[test]
fn converts_legacy_plan_into_section_element_stride_heartbeat_rows() {
    let directory = tempfile::tempdir().unwrap();
    let plan = legacy_plan("pl_legacy");
    let path = legacy_database(directory.path(), &[plan.clone()]);
    let before = legacy_values(&path);
    assert_eq!(
        before["pl_legacy"].as_ref().unwrap(),
        &serde_json::to_value(&plan).unwrap()
    );

    let database = PlanDatabase::open(path.clone()).unwrap();

    let after = loaded_values(&database);
    assert_eq!(after.len(), 1);
    assert!(
        &after["pl_legacy"] == before["pl_legacy"].as_ref().unwrap(),
        "plan differs after the conversion"
    );
    assert_eq!(database.load_plan("pl_legacy").unwrap(), Some(plan));
    for (table, rows) in [
        ("plan_sections", 2),
        ("plan_elements", 1),
        ("plan_strides", 3),
        ("plan_heartbeat", 1),
        ("plan_tasks", 2),
        ("plan_task_milestones", 2),
        ("plan_task_attempts", 3),
    ] {
        assert_eq!(
            count(&path, &format!("SELECT COUNT(*) FROM {table}")),
            rows,
            "{table}"
        );
    }
    let header: String = Connection::open(&path)
        .unwrap()
        .query_row(
            "SELECT header_json FROM plans WHERE id = 'pl_legacy'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    for key in ["tasks", "sections", "elements", "strides", "heartbeat"] {
        assert!(
            !header.contains(&format!("\"{key}\"")),
            "{key} left in header"
        );
    }
    assert!(header.contains("\"planning_state\":\"approved\""));
}

#[test]
fn conversion_runs_once_and_drops_the_legacy_column() {
    let directory = tempfile::tempdir().unwrap();
    let path = legacy_database(directory.path(), &[legacy_plan("pl_once")]);
    let before = legacy_values(&path);
    drop(PlanDatabase::open(path.clone()).unwrap());
    let backup = backup_path(&path);
    let backup_bytes = std::fs::read(&backup).unwrap();
    let backup_modified = std::fs::metadata(&backup).unwrap().modified().unwrap();

    let reopened = PlanDatabase::open(path.clone()).unwrap();
    let mut connection = Connection::open(&path).unwrap();
    assert_eq!(
        convert_legacy_database(&mut connection, &path).unwrap(),
        None
    );
    drop(connection);

    assert_eq!(user_version(&path), SCHEMA_VERSION);
    assert_eq!(
        count(
            &path,
            "SELECT COUNT(*) FROM pragma_table_info('plans') WHERE name = 'document_json'"
        ),
        0
    );
    assert_eq!(
        count(
            &path,
            "SELECT COUNT(*) FROM sqlite_master AS m, pragma_table_info(m.name) AS c
             WHERE m.type = 'table' AND c.name = 'document_json'"
        ),
        0
    );
    assert_eq!(std::fs::read(&backup).unwrap(), backup_bytes);
    assert_eq!(
        std::fs::metadata(&backup).unwrap().modified().unwrap(),
        backup_modified
    );
    let after = loaded_values(&reopened);
    assert!(after["pl_once"] == *before["pl_once"].as_ref().unwrap());
}

#[test]
fn conversion_backs_up_before_altering_and_refuses_without_backup() {
    let directory = tempfile::tempdir().unwrap();
    let path = legacy_database(directory.path(), &[legacy_plan("pl_backup")]);
    let before = legacy_values(&path);
    let backup = backup_path(&path);
    std::fs::create_dir_all(backup.join("occupied")).unwrap();

    let error = PlanDatabase::open(path.clone()).err().unwrap().to_string();

    assert!(error.contains("backup"), "{error}");
    assert_eq!(user_version(&path), 0);
    assert!(has_legacy_plans(&Connection::open(&path).unwrap()).unwrap());
    assert_eq!(legacy_values(&path), before);
    let mut staging = backup.clone().into_os_string();
    staging.push(".partial");
    assert!(!PathBuf::from(staging).exists());

    std::fs::remove_dir_all(&backup).unwrap();
    let database = PlanDatabase::open(path.clone()).unwrap();

    assert!(has_legacy_plans(&Connection::open(&backup).unwrap()).unwrap());
    assert_eq!(user_version(&backup), 0);
    assert_eq!(legacy_values(&backup), before);
    assert!(loaded_values(&database)["pl_backup"] == *before["pl_backup"].as_ref().unwrap());
}

#[test]
fn unparseable_legacy_plan_is_quarantined_and_counted() {
    let directory = tempfile::tempdir().unwrap();
    let good = legacy_plan("pl_good");
    let path = legacy_database(directory.path(), &[good.clone()]);
    let connection = legacy_connection(&path);
    let mut retired_tag = serde_json::to_value(legacy_plan("pl_pane_era")).unwrap();
    retired_tag.as_object_mut().unwrap().remove("tasks");
    retired_tag["planner"] = json!({"kind": "pane", "tab_id": "tab_x"});
    insert_legacy_header(&connection, "pl_pane_era", &retired_tag.to_string());
    insert_legacy_header(&connection, "pl_malformed", "{\"id\":\"pl_malformed\"}");
    connection
        .execute(
            "INSERT INTO specs(id, plan_id, project_path, updated_at, document_json)
             VALUES ('sp_orphaned', 'pl_pane_era', '/repo', 1, '{}')",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO plan_fts(plan_id, title, body) VALUES ('pl_malformed', 'Malformed', '')",
            [],
        )
        .unwrap();
    drop(connection);
    let legacy = legacy_values(&path);
    assert_eq!(legacy.values().filter(|plan| plan.is_err()).count(), 2);

    let mut connection = legacy_connection(&path);
    let report = convert_legacy_database(&mut connection, &path).unwrap();
    drop(connection);

    assert_eq!(
        report,
        Some(ConversionReport {
            converted: 1,
            quarantined: 2
        })
    );
    let database = PlanDatabase::open(path.clone()).unwrap();
    assert_eq!(database.load_snapshots().unwrap().0, vec![good]);
    assert_eq!(count(&path, "SELECT COUNT(*) FROM plans"), 1);
    assert_eq!(count(&path, "SELECT COUNT(*) FROM specs"), 0);
    assert_eq!(
        count(
            &path,
            "SELECT COUNT(*) FROM plan_fts WHERE plan_id = 'pl_malformed'"
        ),
        0
    );
}

fn copy_snapshot(source: &Path, target: &Path) {
    std::fs::copy(source, target).unwrap();
    let mut wal = source.as_os_str().to_owned();
    wal.push("-wal");
    if Path::new(&wal).exists() {
        let mut target_wal = target.as_os_str().to_owned();
        target_wal.push("-wal");
        std::fs::copy(&wal, &target_wal).unwrap();
    }
}

fn column_bytes(path: &Path, columns: &[(&str, &str)]) -> Vec<(String, i64, i64)> {
    let connection = Connection::open(path).unwrap();
    columns
        .iter()
        .map(|(table, column)| {
            let (rows, bytes) = connection
                .query_row(
                    &format!("SELECT COUNT(*), coalesce(sum(length({column})), 0) FROM {table}"),
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            (format!("{table}.{column}"), rows, bytes)
        })
        .collect()
}

fn print_bytes(label: &str, path: &Path, columns: &[(String, i64, i64)]) {
    println!(
        "{label}: file_bytes={}",
        std::fs::metadata(path).unwrap().len()
    );
    for (column, rows, bytes) in columns {
        println!("  {column:<34} rows={rows:>7} bytes={bytes:>10}");
    }
}

#[test]
#[ignore = "reads a production snapshot copy named by DOMIOS_PLAN_SNAPSHOT"]
fn production_snapshot_round_trips_after_conversion() {
    let source = PathBuf::from(
        std::env::var(SNAPSHOT_ENV).expect("DOMIOS_PLAN_SNAPSHOT names a plans.db copy"),
    );
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("plans.db");
    copy_snapshot(&source, &path);

    let before_bytes = column_bytes(
        &path,
        &[
            ("plans", "document_json"),
            ("plan_tasks", "document_json"),
            ("plan_task_milestones", "document_json"),
            ("plan_task_attempts", "document_json"),
            ("specs", "document_json"),
        ],
    );
    let legacy = legacy_values(&path);
    let legacy_quarantined = legacy.values().filter(|plan| plan.is_err()).count();
    let legacy_specs: BTreeMap<String, Value> = {
        let connection = Connection::open(&path).unwrap();
        read_legacy_specs(&connection)
            .unwrap()
            .into_iter()
            .filter_map(|spec| {
                let parsed: Spec = serde_json::from_str(&spec.document).ok()?;
                Some((spec.id, serde_json::to_value(parsed).unwrap()))
            })
            .collect()
    };
    let size_before = std::fs::metadata(&path).unwrap().len();

    let mut connection = legacy_connection(&path);
    let report = convert_legacy_database(&mut connection, &path)
        .unwrap()
        .expect("the snapshot is a 1.17 plans.db");
    drop(connection);
    let database = PlanDatabase::open(path.clone()).unwrap();
    let after = loaded_values(&database);
    let after_specs: BTreeMap<String, Value> = database
        .load_snapshots()
        .unwrap()
        .1
        .into_iter()
        .map(|spec| (spec.id.clone(), serde_json::to_value(spec).unwrap()))
        .collect();

    println!(
        "plans={} converted={} quarantined legacy={legacy_quarantined} conversion={}",
        legacy.len(),
        report.converted,
        report.quarantined
    );
    assert_eq!(report.quarantined, legacy_quarantined);
    assert_eq!(report.converted, after.len());
    let mut planning_states: BTreeMap<String, usize> = BTreeMap::new();
    for (id, plan) in &legacy {
        let Ok(plan) = plan else { continue };
        assert!(
            after.get(id) == Some(plan),
            "plan {id} differs after the 1.18 conversion"
        );
        *planning_states
            .entry(plan["planning_state"].to_string())
            .or_default() += 1;
    }
    println!("planning_state equal per plan: {planning_states:?}");
    assert_eq!(after_specs, legacy_specs);
    println!("specs equal: {}", after_specs.len());

    println!("legacy file_bytes={size_before}");
    print_bytes("before (1.17)", &backup_path(&path), &before_bytes);
    let after_bytes = column_bytes(
        &path,
        &[
            ("plans", "header_json"),
            ("plan_sections", "row_json"),
            ("plan_elements", "row_json"),
            ("plan_strides", "row_json"),
            ("plan_heartbeat", "row_json"),
            ("plan_tasks", "row_json"),
            ("plan_task_milestones", "row_json"),
            ("plan_task_attempts", "row_json"),
            ("specs", "row_json"),
        ],
    );
    print_bytes("after (1.18)", &path, &after_bytes);
}
