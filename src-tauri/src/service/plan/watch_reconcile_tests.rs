use super::*;
use crate::plan::{OrchestratorRef, Plan, PlanStatus};

fn new_plan(root: &std::path::Path, title: &str) -> Plan {
    Plan::new(
        title.into(),
        root.display().to_string(),
        OrchestratorRef::from_pane("abc"),
    )
}

fn plan_json(root: &std::path::Path, plan: &Plan) -> std::path::PathBuf {
    let project = paths::project_root(root);
    paths::plan_json_path(&project, &plan.status, &plan.id).unwrap()
}

// External process writes a plan.json for `plan` at `updated_at`, changing mtime.
fn external_write(root: &std::path::Path, plan: &Plan, title: &str, updated_at: u64) {
    let mut copy = plan.clone();
    copy.title = title.into();
    copy.updated_at = updated_at;
    let path = plan_json(root, &copy);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, serde_json::to_string_pretty(&copy).unwrap()).unwrap();
    // Force a distinct mtime so is_own_recent_write cannot mistake it for ours.
    filetime::set_file_mtime(&path, filetime::FileTime::from_unix_time(updated_at as i64, 0))
        .ok();
}

#[test]
fn external_delete_removes_index_entry_and_signals() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let project = paths::project_root(dir.path());
    let mut plan = new_plan(dir.path(), "Doomed");
    store.save(&mut plan, 1000).unwrap();
    assert!(store.get(&plan.id).is_some());

    // Another daemon deletes the plan folder on disk directly.
    std::fs::remove_dir_all(plan_json(dir.path(), &plan).parent().unwrap()).unwrap();

    let outcome = store.reconcile_watched_plan(&project, &plan.id).unwrap();
    assert_eq!(outcome, WatchOutcome::Removed);
    assert!(outcome.should_emit());
    assert!(store.get(&plan.id).is_none());
    assert!(!store.list_index().iter().any(|e| e.plan_id == plan.id));
}

#[test]
fn external_add_appears_in_index() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let project = paths::project_root(dir.path());
    let plan = new_plan(dir.path(), "Freshly added");
    external_write(dir.path(), &plan, "Freshly added", 2000);
    assert!(store.get(&plan.id).is_none());

    let outcome = store.reconcile_watched_plan(&project, &plan.id).unwrap();
    assert_eq!(outcome, WatchOutcome::Loaded);
    assert!(outcome.should_emit());
    let loaded = store.get(&plan.id).unwrap();
    assert_eq!(loaded.title, "Freshly added");
}

#[test]
fn external_rewrite_updates_store() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let project = paths::project_root(dir.path());
    let mut plan = new_plan(dir.path(), "Original");
    store.save(&mut plan, 1000).unwrap();

    external_write(dir.path(), &plan, "Rewritten externally", 3000);
    let outcome = store.reconcile_watched_plan(&project, &plan.id).unwrap();
    assert_eq!(outcome, WatchOutcome::Loaded);
    assert_eq!(store.get(&plan.id).unwrap().title, "Rewritten externally");
    assert_eq!(store.get(&plan.id).unwrap().updated_at, 3000);
}

#[test]
fn self_write_is_suppressed() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let project = paths::project_root(dir.path());
    let mut plan = new_plan(dir.path(), "Ours");
    store.save(&mut plan, 1000).unwrap();

    // No external mutation since our save -> reconcile must be a no-op.
    let outcome = store.reconcile_watched_plan(&project, &plan.id).unwrap();
    assert_eq!(outcome, WatchOutcome::Unchanged);
    assert!(!outcome.should_emit());
    assert_eq!(store.get(&plan.id).unwrap().title, "Ours");
}

#[test]
fn in_flight_memory_wins_over_stale_disk_read() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let project = paths::project_root(dir.path());
    let mut plan = new_plan(dir.path(), "In-flight newer");
    store.save(&mut plan, 5000).unwrap();

    // A stale external write with an OLDER generation must not clobber memory.
    external_write(dir.path(), &plan, "Stale disk copy", 1000);
    let outcome = store.reconcile_watched_plan(&project, &plan.id).unwrap();
    assert_eq!(outcome, WatchOutcome::Loaded);
    assert_eq!(store.get(&plan.id).unwrap().title, "In-flight newer");
    assert_eq!(store.get(&plan.id).unwrap().updated_at, 5000);
}

#[test]
fn reconcile_absent_plan_is_noop() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let project = paths::project_root(dir.path());
    let outcome = store.reconcile_watched_plan(&project, "pl_never_existed").unwrap();
    assert_eq!(outcome, WatchOutcome::Absent);
    assert!(!outcome.should_emit());
}
