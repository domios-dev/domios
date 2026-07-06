use super::*;
use crate::plan::{OrchestratorRef, Plan};

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

fn plan_yaml(root: &std::path::Path, plan: &Plan) -> std::path::PathBuf {
    let project = paths::project_root(root);
    let dir = paths::plan_dir(&project, &plan.status, &plan.id).unwrap();
    store_v2::plan_yaml_path(&dir)
}

// External process writes plan.yaml for `plan` at `updated_at`, changing mtime.
fn external_write(root: &std::path::Path, plan: &Plan, title: &str, updated_at: u64) {
    let mut copy = plan.clone();
    copy.title = title.into();
    copy.updated_at = updated_at;
    let project = paths::project_root(root);
    let dir = paths::plan_dir(&project, &copy.status, &copy.id).unwrap();
    store_v2::write_plan(&dir, &copy).unwrap();
    let path = store_v2::plan_yaml_path(&dir);
    // Force a distinct mtime so is_own_recent_write cannot mistake it for ours.
    filetime::set_file_mtime(
        &path,
        filetime::FileTime::from_unix_time(updated_at as i64, 0),
    )
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
    let mut plan = new_plan(dir.path(), "Ours");
    store.save(&mut plan, 1000).unwrap();

    let yaml_path = plan_yaml(dir.path(), &store.get(&plan.id).unwrap());
    assert!(
        store.is_own_recent_write(&yaml_path),
        "v2 save must register plan.yaml as an own write"
    );
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
fn v2_plan_survives_when_legacy_json_absent() {
    // Regression: a v2 plan persists only plan.yaml (no plan.json). A watcher
    // burst — e.g. the one-off conversion deleting the legacy json — must NOT
    // evict it. The old json-only discovery returned None here and forgot it.
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let project = paths::project_root(dir.path());
    let mut plan = new_plan(dir.path(), "V2 only");
    store.save(&mut plan, 1000).unwrap();
    assert!(plan_yaml(dir.path(), &plan).is_file());
    assert!(!plan_json(dir.path(), &plan).is_file());

    let outcome = store.reconcile_watched_plan(&project, &plan.id).unwrap();
    assert_ne!(outcome, WatchOutcome::Removed);
    assert!(
        store.get(&plan.id).is_some(),
        "v2 plan must survive when only the legacy json is gone"
    );
    assert_eq!(store.get(&plan.id).unwrap().title, "V2 only");
}

#[test]
fn reconcile_absent_plan_is_noop() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let project = paths::project_root(dir.path());
    let outcome = store
        .reconcile_watched_plan(&project, "pl_never_existed")
        .unwrap();
    assert_eq!(outcome, WatchOutcome::Absent);
    assert!(!outcome.should_emit());
}
