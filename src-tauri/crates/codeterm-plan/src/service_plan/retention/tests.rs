use super::*;
use crate::plan::{Milestone, OrchestratorRef, TaskExecutionAttempt};
use crate::service_plan::search_index::{flush_index_queue_for_test, project_index_path};
use crate::service_plan::NewTask;
use rusqlite::Connection;

const NOW_MS: u64 = 2_000_000_000_000;

fn plan_with_history(
    store: &PlanStore,
    root: &std::path::Path,
    title: &str,
    age_days: u64,
) -> String {
    let at = NOW_MS - age_days * DAY_MS;
    let plan = store
        .create_plan(
            title.into(),
            root.display().to_string(),
            OrchestratorRef::from_tab("orch"),
            vec![],
            at,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: format!("{title} task"),
                body: format!("{title} body"),
                ..Default::default()
            },
            at,
        )
        .unwrap();
    store
        .with_plan(&plan.id, at, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            task.milestones.push(Milestone {
                id: format!("{title}-milestone"),
                from: OrchestratorRef::from_tab("orch"),
                kind: "note".into(),
                text: "history".into(),
                ts: at,
                links: vec![],
                loop_id: None,
                iteration: None,
                canvas: Default::default(),
                attachments: vec![],
            });
            task.execution_attempts.push(TaskExecutionAttempt {
                id: format!("{title}-attempt"),
                executor_tab_id: "worker".into(),
                ..Default::default()
            });
            Ok(())
        })
        .unwrap();
    let mut spec = store
        .create_spec(format!("{title} spec"), root.display().to_string(), at)
        .unwrap();
    spec.plan_id = Some(plan.id.clone());
    store.save_spec(&mut spec, at).unwrap();
    store.mirror_spec_index(&spec).unwrap();
    plan.id
}

fn stored_rows(database: &std::path::Path, plan_id: &str) -> Vec<i64> {
    let connection = Connection::open(database).unwrap();
    [
        "SELECT COUNT(*) FROM plans WHERE id = ?1",
        "SELECT COUNT(*) FROM specs WHERE plan_id = ?1",
        "SELECT COUNT(*) FROM plan_tasks WHERE plan_id = ?1",
        "SELECT COUNT(*) FROM plan_task_milestones WHERE plan_id = ?1",
        "SELECT COUNT(*) FROM plan_task_attempts WHERE plan_id = ?1",
        "SELECT COUNT(*) FROM plan_fts WHERE plan_id = ?1",
    ]
    .iter()
    .map(|sql| {
        connection
            .query_row(sql, [plan_id], |row| row.get(0))
            .unwrap()
    })
    .collect()
}

fn index_units(root: &std::path::Path, ids: &[&str]) -> i64 {
    let connection = Connection::open(project_index_path(root)).unwrap();
    ids.iter()
        .map(|id| {
            connection
                .query_row(
                    "SELECT COUNT(*) FROM mem_items WHERE id LIKE ?1",
                    [format!("planunit_{id}_%")],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap()
        })
        .sum()
}

#[test]
fn a_plan_older_than_a_year_is_removed_with_all_its_rows_and_a_younger_one_stays() {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let database = store.database_path_for_test();
    let stale = plan_with_history(&store, root.path(), "Stale", 366);
    let fresh = plan_with_history(&store, root.path(), "Fresh", 364);
    let stale_spec = store
        .list_specs_full()
        .into_iter()
        .find(|spec| spec.plan_id.as_deref() == Some(stale.as_str()))
        .unwrap()
        .id;
    flush_index_queue_for_test(root.path());
    assert_eq!(stored_rows(&database, &stale), [1, 1, 1, 1, 1, 1]);
    assert!(index_units(root.path(), &[&stale, &stale_spec]) > 0);

    let pruned = store.prune_stale_plans(NOW_MS);
    flush_index_queue_for_test(root.path());

    assert_eq!(
        pruned,
        [PrunedPlan {
            id: stale.clone(),
            title: "Stale".into(),
            updated_at: NOW_MS - 366 * DAY_MS,
        }]
    );
    assert!(store.get(&stale).is_none());
    assert_eq!(stored_rows(&database, &stale), [0, 0, 0, 0, 0, 0]);
    assert_eq!(index_units(root.path(), &[&stale, &stale_spec]), 0);
    assert!(store.get(&fresh).is_some());
    assert_eq!(stored_rows(&database, &fresh), [1, 1, 1, 1, 1, 1]);
    assert!(index_units(root.path(), &[&fresh]) > 0);
}

#[test]
fn retention_claims_each_day_once() {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());

    assert!(store.claim_retention_day(NOW_MS));
    assert!(!store.claim_retention_day(NOW_MS + 1));
    assert!(store.claim_retention_day(NOW_MS + DAY_MS));
}

#[test]
fn a_plan_touched_after_the_candidate_scan_is_not_deleted() {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let database = store.database_path_for_test();
    let touched = plan_with_history(&store, root.path(), "Touched", 400);
    let cutoff = NOW_MS - PLAN_RETENTION_MS;
    store
        .with_plan(&touched, NOW_MS, |plan| {
            plan.tasks[0].body = "touched".into();
            Ok(())
        })
        .unwrap();

    assert!(store
        .delete_if_untouched_since(&touched, cutoff)
        .unwrap()
        .is_none());
    assert!(store.get(&touched).is_some());
    assert_eq!(stored_rows(&database, &touched), [1, 1, 1, 1, 1, 1]);
}
