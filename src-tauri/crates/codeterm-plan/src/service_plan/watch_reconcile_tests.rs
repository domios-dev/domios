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
fn missing_legacy_sidecar_reloads_canonical_database_plan() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let project = paths::project_root(dir.path());
    let mut plan = new_plan(dir.path(), "Database backed");
    store.save(&mut plan, 1000).unwrap();
    assert!(store.get(&plan.id).is_some());

    let outcome = store.reconcile_watched_plan(&project, &plan.id).unwrap();
    assert_eq!(outcome, WatchOutcome::Unchanged);
    assert!(!outcome.should_emit());
    assert_eq!(store.get(&plan.id).unwrap().title, "Database backed");
    assert!(store.list_index().iter().any(|e| e.plan_id == plan.id));
}

#[test]
fn watcher_rehydrates_database_plan_missing_from_runtime() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let project = paths::project_root(dir.path());
    let mut plan = new_plan(dir.path(), "Recover me");
    store.save(&mut plan, 1000).unwrap();
    store.forget_plan(&plan.id);
    assert!(store.get(&plan.id).is_none());

    let outcome = store.reconcile_watched_plan(&project, &plan.id).unwrap();
    assert_eq!(outcome, WatchOutcome::Loaded);
    assert!(outcome.should_emit());
    assert_eq!(store.get(&plan.id).unwrap().title, "Recover me");
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

fn bump_mtime(path: &std::path::Path) {
    filetime::set_file_mtime(path, filetime::FileTime::from_unix_time(9_999_999_999, 0)).unwrap();
}

#[test]
fn hand_edited_task_yaml_reconciles_while_plan_yaml_is_own_write() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let project = paths::project_root(dir.path());
    let mut plan = new_plan(dir.path(), "Tasks");
    store.save(&mut plan, 1000).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            crate::service_plan::NewTask {
                name: "hand edit me".into(),
                body: "original body line.\nsecond line.".into(),
                ..Default::default()
            },
            1001,
        )
        .unwrap();

    // Hand-edit ONLY the task yaml; plan.yaml stays our own recent write.
    let plan_dir = store_v2::plan_state_dir(&store.get(&plan.id).unwrap()).unwrap();
    let task_yaml = plan_dir.join("tasks/hand-edit-me.yaml");
    let text = std::fs::read_to_string(&task_yaml).unwrap();
    std::fs::write(&task_yaml, text.replace("original body", "EDITED body")).unwrap();
    bump_mtime(&task_yaml);

    let outcome = store.reconcile_watched_plan(&project, &plan.id).unwrap();
    assert_eq!(outcome, WatchOutcome::Loaded);
    let task = store
        .get(&plan.id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|t| t.id == task_id)
        .unwrap();
    assert!(task.body.contains("EDITED body"), "got: {}", task.body);
}

#[test]
fn hand_edited_co_located_spec_yaml_reconciles() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let project = paths::project_root(dir.path());
    let spec = store
        .create_spec("Reqs".into(), dir.path().display().to_string(), 1)
        .unwrap();
    store
        .add_spec_section(
            &spec.id,
            "Req",
            "spec body one.\nspec body two.",
            None,
            &[],
            None,
            None,
            2,
        )
        .unwrap();
    let plan = store
        .create_plan_from_spec(
            &spec.id,
            "Plan".into(),
            OrchestratorRef::default(),
            vec![],
            3,
        )
        .unwrap();

    let plan_dir = store_v2::plan_state_dir(&store.get(&plan.id).unwrap()).unwrap();
    let spec_yaml = plan_dir.join("spec.yaml");
    let text = std::fs::read_to_string(&spec_yaml).unwrap();
    std::fs::write(
        &spec_yaml,
        text.replace("spec body one", "HAND-EDITED spec body"),
    )
    .unwrap();
    bump_mtime(&spec_yaml);

    let outcome = store.reconcile_watched_plan(&project, &plan.id).unwrap();
    assert_eq!(outcome, WatchOutcome::Loaded);
    let loaded = store.get_spec(&spec.id).unwrap();
    assert!(
        loaded.sections[0].body.contains("HAND-EDITED spec body"),
        "got: {}",
        loaded.sections[0].body
    );
}

#[test]
fn untouched_plan_dir_stays_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let project = paths::project_root(dir.path());
    let mut plan = new_plan(dir.path(), "Quiet");
    store.save(&mut plan, 1000).unwrap();
    store
        .add_task(
            &plan.id,
            crate::service_plan::NewTask {
                name: "quiet task".into(),
                ..Default::default()
            },
            1001,
        )
        .unwrap();

    let outcome = store.reconcile_watched_plan(&project, &plan.id).unwrap();
    assert_eq!(outcome, WatchOutcome::Unchanged);
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
