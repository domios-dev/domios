use super::*;
use crate::plan::OrchestratorRef;
use crate::service_plan::PlanStore;

#[test]
fn row_jobs_for_all_plans_precede_embedding_backlog() {
    let root = tempfile::tempdir().unwrap();
    let _store = PlanStore::new_for_test(root.path());
    let plan = Plan::new(
        "First".into(),
        root.path().display().to_string(),
        OrchestratorRef::default(),
    );
    let second = Plan::new(
        "Second".into(),
        root.path().display().to_string(),
        OrchestratorRef::default(),
    );
    let queue = IndexQueue {
        state: Mutex::new(IndexQueueState::default()),
        ready: Condvar::new(),
    };
    for unit in plan_units(&plan, &project_scope(root.path())) {
        queue.enqueue(IndexJob::Embed(root.path().to_path_buf(), unit));
    }
    queue.enqueue(IndexJob::Plan(second.clone(), PlanIndexScope::Full));
    let (key, project, job) = queue.take();
    assert!(matches!(job, IndexJob::Plan(ref snapshot, _) if snapshot.id == second.id));
    queue.complete(key, project, false);
    assert!(matches!(queue.take().2, IndexJob::Embed(..)));
}

#[test]
fn queued_embedding_cannot_recreate_deleted_units() {
    let root = tempfile::tempdir().unwrap();
    let _store = PlanStore::new_for_test(root.path());
    let plan = Plan::new(
        "Deleted".into(),
        root.path().display().to_string(),
        OrchestratorRef::default(),
    );
    let units = mirror_plan(&plan, false).unwrap();
    delete_plan_units(&plan).unwrap();
    for unit in units {
        embed_current_with(root.path(), &unit, |_, _| {
            panic!("deleted row reached embedder")
        })
        .unwrap();
    }
    let conn = open_index(root.path()).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM mem_items", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn embedding_failure_names_the_affected_plan_unit() {
    let root = tempfile::tempdir().unwrap();
    let _store = PlanStore::new_for_test(root.path());
    let plan = Plan::new(
        "Failure".into(),
        root.path().display().to_string(),
        OrchestratorRef::default(),
    );
    let units = mirror_plan(&plan, false).unwrap();
    let unit = &units[0];
    let error =
        embed_current_with(root.path(), unit, |_, _| Err("model unavailable".into())).unwrap_err();
    assert!(error.contains(&plan.id));
    assert!(error.contains("model unavailable"));
}
