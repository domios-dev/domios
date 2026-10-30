use super::*;
use crate::plan::TaskStatus;
use std::collections::BTreeMap;
use std::time::Instant;

const SNAPSHOT_ENV: &str = "DOMIOS_PLAN_SNAPSHOT";
const TARGET_ENV: &str = "DOMIOS_PLAN_SNAPSHOT_PLAN";

fn loaded(database: &PlanDatabase) -> BTreeMap<String, serde_json::Value> {
    database
        .load_plans_for_test()
        .into_iter()
        .map(|plan| (plan.id.clone(), serde_json::to_value(plan).unwrap()))
        .collect()
}

fn table_rows(path: &std::path::Path) -> Vec<(&'static str, i64)> {
    let connection = Connection::open(path).unwrap();
    [
        "plans",
        "plan_tasks",
        "plan_task_milestones",
        "plan_task_attempts",
        "specs",
    ]
    .into_iter()
    .map(|table| {
        let count = connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        (table, count)
    })
    .collect()
}

fn plan_row_sizes(path: &std::path::Path) -> Vec<(String, i64)> {
    let connection = Connection::open(path).unwrap();
    let mut statement = connection
        .prepare("SELECT id, length(header_json) FROM plans ORDER BY 2 DESC")
        .unwrap();
    let rows = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap();
    rows.map(Result::unwrap).collect()
}

fn task_bytes(plan: &Plan) -> Vec<(String, String)> {
    plan.tasks
        .iter()
        .map(|task| (task.id.clone(), serde_json::to_string(task).unwrap()))
        .collect()
}

#[test]
#[ignore = "reads a production snapshot named by DOMIOS_PLAN_SNAPSHOT"]
fn production_snapshot_current_rows_survive_mutation_and_reopen() {
    let source = std::path::PathBuf::from(
        std::env::var(SNAPSHOT_ENV).expect("DOMIOS_PLAN_SNAPSHOT names a plans.db copy"),
    );
    let target = std::env::var(TARGET_ENV).unwrap_or_else(|_| "pl_7lsisa".into());
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("plans.db");
    std::fs::copy(&source, &path).unwrap();
    let source_bytes = std::fs::metadata(&path).unwrap().len();

    let started = Instant::now();
    let database = PlanDatabase::open(path.clone()).unwrap();
    let open = started.elapsed();
    let started = Instant::now();
    let expected = loaded(&database);
    let load = started.elapsed();
    let plans = database.load_plans_for_test();
    let tasks: usize = plans.iter().map(|plan| plan.tasks.len()).sum();
    let milestones: usize = plans
        .iter()
        .flat_map(|plan| &plan.tasks)
        .map(|task| task.milestones.len())
        .sum();
    let attempts: usize = plans
        .iter()
        .flat_map(|plan| &plan.tasks)
        .map(|task| task.execution_attempts.len())
        .sum();
    println!("snapshot bytes={source_bytes} plans={} tasks={tasks} milestones={milestones} attempts={attempts}", plans.len());
    println!(
        "open_ms={} load_all_ms={}",
        open.as_millis(),
        load.as_millis()
    );
    println!("rows {:?}", table_rows(&path));
    let sizes = plan_row_sizes(&path);
    println!(
        "plan row bytes max={:?} {target}={:?}",
        sizes.first(),
        sizes
            .iter()
            .find(|(id, _)| *id == target)
            .map(|(_, size)| size)
    );

    let mut plan = plans.into_iter().find(|plan| plan.id == target).unwrap();
    let before = task_bytes(&plan);
    let with_history = plan
        .tasks
        .iter()
        .position(|task| !task.milestones.is_empty())
        .unwrap();
    let mut milestone = plan.tasks[with_history].milestones[0].clone();
    milestone.id = "snapshot-check-milestone".into();
    milestone.text = "snapshot check".into();
    plan.tasks[with_history].milestones.push(milestone);
    let restatus = (with_history + 1) % plan.tasks.len();
    plan.tasks[restatus].status = if plan.tasks[restatus].status == TaskStatus::Blocked {
        TaskStatus::Pending
    } else {
        TaskStatus::Blocked
    };
    plan.updated_at += 1;
    crate::service_plan::text_bounds::validate_plan(&plan).unwrap();
    let started = Instant::now();
    let writes = database.persist_plan_rows(&plan, None).unwrap();
    println!(
        "mutation persist_ms={} writes={writes:?}",
        started.elapsed().as_millis()
    );
    let changed = [
        plan.tasks[with_history].id.clone(),
        plan.tasks[restatus].id.clone(),
    ];
    assert_eq!(writes.changed_tasks.len(), 2);
    assert!(changed.iter().all(|id| writes.changed_tasks.contains(id)));
    assert_eq!(
        (
            writes.tasks,
            writes.milestones,
            writes.attempts,
            writes.deleted
        ),
        (1, 1, 0, 0)
    );

    let reloaded = database.load_plan(&target).unwrap().unwrap();
    assert!(reloaded == plan, "mutated plan differs after reload");
    for ((id, bytes), (reloaded_id, reloaded_bytes)) in before.iter().zip(task_bytes(&reloaded)) {
        assert_eq!(*id, reloaded_id);
        if !changed.contains(id) {
            assert!(*bytes == reloaded_bytes, "untouched task {id} changed");
        }
    }
    let rows_after_mutation = table_rows(&path);
    drop(database);

    let started = Instant::now();
    let reopened = PlanDatabase::open(path.clone()).unwrap();
    println!("second open_ms={}", started.elapsed().as_millis());
    assert_eq!(table_rows(&path), rows_after_mutation);
    let mut expected = expected;
    expected.insert(target.clone(), serde_json::to_value(&plan).unwrap());
    let actual = loaded(&reopened);
    assert_eq!(actual.len(), expected.len());
    for (id, plan) in &expected {
        assert!(
            actual[id] == *plan,
            "plan {id} differs after the second open"
        );
    }
    println!(
        "verified {} plans after the idempotent second open",
        actual.len()
    );
}
