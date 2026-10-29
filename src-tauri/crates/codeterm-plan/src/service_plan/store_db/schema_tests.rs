use super::*;
use crate::plan::{Milestone, OrchestratorRef, Task, TaskExecutionAttempt, TaskStatus};

fn milestone(id: &str, text_bytes: usize) -> Milestone {
    Milestone {
        id: id.into(),
        from: OrchestratorRef::from_tab("worker"),
        kind: "note".into(),
        text: "m".repeat(text_bytes),
        ts: 5,
        links: vec![],
        loop_id: None,
        iteration: None,
        canvas: Default::default(),
        attachments: vec![],
    }
}

fn task(id: &str, milestones: usize, attempts: usize, text_bytes: usize) -> Task {
    let mut task: Task = serde_json::from_value(serde_json::json!({
        "id": id, "name": format!("Task {id}"), "type": "implement", "status": "pending"
    }))
    .unwrap();
    task.milestones = (0..milestones)
        .map(|seq| milestone(&format!("{id}-m{seq}"), text_bytes))
        .collect();
    task.execution_attempts = (0..attempts)
        .map(|seq| TaskExecutionAttempt {
            id: format!("{id}-a{seq}"),
            executor_tab_id: format!("tab-{seq}"),
            ..Default::default()
        })
        .collect();
    task
}

fn plan(tasks: Vec<Task>) -> Plan {
    let mut plan = Plan::new(
        "Row plan".into(),
        "D:/project".into(),
        OrchestratorRef::from_tab("orch"),
    );
    plan.id = "pl_rows".into();
    plan.updated_at = 3;
    plan.tasks = tasks;
    plan
}

fn database() -> (tempfile::TempDir, PathBuf, PlanDatabase) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("plans.db");
    let database = PlanDatabase::open(path.clone()).unwrap();
    (directory, path, database)
}

fn row_counts(path: &std::path::Path) -> [i64; 3] {
    let connection = Connection::open(path).unwrap();
    ["plan_tasks", "plan_task_milestones", "plan_task_attempts"].map(|table| {
        connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    })
}

fn insert_whole_document(path: &std::path::Path, plan: &Plan) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute(
            "INSERT INTO plans(id, title, status, team_id, project_path, updated_at, document_json)
             VALUES (?1, ?2, 'active', NULL, ?3, ?4, ?5)",
            params![
                plan.id,
                plan.title,
                plan.project_path,
                plan.updated_at,
                serde_json::to_string(plan).unwrap()
            ],
        )
        .unwrap();
}

fn plan_row(path: &std::path::Path, id: &str) -> String {
    Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT document_json FROM plans WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn a_large_plan_round_trips_through_rows_in_order() {
    let (_directory, path, database) = database();
    let tasks = (0..60)
        .rev()
        .map(|seq| task(&format!("t{seq:02}"), seq % 7, seq % 4, 64))
        .collect();
    let plan = plan(tasks);

    database.persist_plan(&plan, None).unwrap();
    drop(database);

    let reopened = PlanDatabase::open(path.clone()).unwrap();
    assert_eq!(reopened.load_snapshots().unwrap().0, vec![plan.clone()]);
    assert_eq!(reopened.load_plan(&plan.id).unwrap(), Some(plan));
    assert!(!plan_row(&path, "pl_rows").contains("\"tasks\""));
}

#[test]
fn a_plan_far_above_sixteen_mebibytes_persists() {
    let (_directory, path, database) = database();
    let tasks = (0..20)
        .map(|seq| task(&format!("big{seq}"), 4, 1, 250 * 1024))
        .collect();
    let plan = plan(tasks);
    assert!(serde_json::to_string(&plan).unwrap().len() > 18 * 1024 * 1024);
    crate::service_plan::text_bounds::validate_plan(&plan).unwrap();

    database.persist_plan(&plan, None).unwrap();

    assert_eq!(row_counts(&path), [20, 80, 20]);
    assert_eq!(database.load_plan(&plan.id).unwrap(), Some(plan));
}

#[test]
fn changing_one_task_writes_only_that_tasks_rows() {
    let (_directory, _path, database) = database();
    let mut plan = plan(
        (0..30)
            .map(|seq| task(&format!("t{seq}"), 3, 2, 32))
            .collect(),
    );
    let first = database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!(
        (first.tasks, first.milestones, first.attempts),
        (30, 90, 60)
    );

    plan.tasks[7].status = TaskStatus::InProgress;
    plan.updated_at = 4;
    let status = database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!(
        (
            status.tasks,
            status.milestones,
            status.attempts,
            status.deleted
        ),
        (1, 0, 0, 0)
    );
    assert_eq!(status.changed_tasks, ["t7"]);

    plan.tasks[9].milestones.push(milestone("t9-new", 8));
    plan.tasks[9].execution_attempts[1].executor_title = Some("renamed".into());
    let appended = database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!(
        (
            appended.tasks,
            appended.milestones,
            appended.attempts,
            appended.deleted
        ),
        (0, 1, 1, 0)
    );
    assert_eq!(appended.changed_tasks, ["t9"]);

    plan.tasks[9].milestones[0].text = "edited history".into();
    let rewritten = database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!((rewritten.milestones, rewritten.deleted), (4, 4));
    assert_eq!(database.load_plan(&plan.id).unwrap(), Some(plan.clone()));

    let unchanged = database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!(
        (unchanged.tasks, unchanged.milestones, unchanged.attempts),
        (0, 0, 0)
    );
    assert!(unchanged.changed_tasks.is_empty());
    assert!(!unchanged.header_identity_changed);

    plan.title = "Renamed plan".into();
    assert!(
        database
            .persist_plan_rows(&plan, None)
            .unwrap()
            .header_identity_changed
    );
}

#[test]
fn removing_a_task_or_attempt_removes_its_rows_and_reorders_survivors() {
    let (_directory, path, database) = database();
    let mut plan = plan(vec![
        task("a", 2, 2, 8),
        task("b", 3, 1, 8),
        task("c", 1, 3, 8),
    ]);
    database.persist_plan(&plan, None).unwrap();
    assert_eq!(row_counts(&path), [3, 6, 6]);

    plan.tasks.remove(1);
    plan.tasks.reverse();
    plan.tasks[0].execution_attempts.remove(0);
    plan.tasks[0].execution_attempts.reverse();
    let writes = database.persist_plan_rows(&plan, None).unwrap();

    assert_eq!(writes.deleted, 2);
    assert_eq!(row_counts(&path), [2, 3, 4]);
    assert_eq!(database.load_plan(&plan.id).unwrap(), Some(plan));

    database.delete_plan("pl_rows", 9).unwrap();
    assert_eq!(row_counts(&path), [0, 0, 0]);
}

#[test]
fn opening_a_database_never_rewrites_whole_documents() {
    let (_directory, path, database) = database();
    drop(database);
    let tasks = (0..18)
        .map(|seq| task(&format!("old{seq}"), 4, 2, 250 * 1024))
        .collect();
    let legacy = plan(tasks);
    insert_whole_document(&path, &legacy);
    assert!(plan_row(&path, &legacy.id).len() > 17 * 1024 * 1024);

    let original = plan_row(&path, &legacy.id);
    let migrated = PlanDatabase::open(path.clone()).unwrap();
    let after_first = (plan_row(&path, &legacy.id), row_counts(&path));
    assert_eq!(after_first.0, original);
    assert_eq!(after_first.1, [0, 0, 0]);
    assert_eq!(migrated.load_snapshots().unwrap().0, vec![legacy.clone()]);
    drop(migrated);

    let reopened = PlanDatabase::open(path.clone()).unwrap();
    assert_eq!(
        (plan_row(&path, &legacy.id), row_counts(&path)),
        after_first
    );
    assert_eq!(reopened.load_plan(&legacy.id).unwrap(), Some(legacy));
}

#[test]
fn a_plan_with_duplicate_task_ids_stays_writable_as_a_whole_document() {
    let (_directory, path, database) = database();
    let mut plan = plan(
        (0..20)
            .map(|seq| task(&format!("big{seq}"), 1, 1, 250 * 1024))
            .collect(),
    );
    database.persist_plan(&plan, None).unwrap();
    plan.tasks[1].id = plan.tasks[0].id.clone();
    assert!(serde_json::to_string(&plan).unwrap().len() > store_rows::MAX_STORED_ROW_BYTES);

    let writes = database.persist_plan_rows(&plan, None).unwrap();

    assert!(writes.header_identity_changed);
    assert_eq!(row_counts(&path), [0, 0, 0]);
    assert!(plan_row(&path, &plan.id).contains("\"tasks\""));
    assert_eq!(database.load_plan(&plan.id).unwrap(), Some(plan.clone()));

    plan.tasks[1].id = "big1".into();
    database.persist_plan(&plan, None).unwrap();
    assert_eq!(row_counts(&path), [20, 20, 20]);
    assert!(!plan_row(&path, &plan.id).contains("\"tasks\""));
    assert_eq!(database.load_plan(&plan.id).unwrap(), Some(plan));
}

#[test]
fn an_unsplittable_whole_document_is_kept_intact_and_still_loads() {
    let (_directory, path, database) = database();
    drop(database);
    let mut legacy = plan(vec![task("dup", 1, 0, 8), task("dup", 1, 0, 8)]);
    legacy.id = "pl_dup".into();
    insert_whole_document(&path, &legacy);
    let source = plan_row(&path, &legacy.id);

    let reopened = PlanDatabase::open(path.clone()).unwrap();

    assert_eq!(plan_row(&path, &legacy.id), source);
    assert_eq!(row_counts(&path), [0, 0, 0]);
    assert_eq!(reopened.load_plan(&legacy.id).unwrap(), Some(legacy));
}
