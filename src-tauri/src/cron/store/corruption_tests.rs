use super::*;

fn params(name: &str) -> CronCreateParams {
    serde_json::from_value(serde_json::json!({
        "name": name,
        "workspace_id": "ws",
        "schedule": "0 * * * *",
        "task": "test task",
        "agent": "claude",
        "jitter_max_ms": 0,
    }))
    .unwrap()
}

fn names(store: &CronStore) -> Vec<String> {
    let mut names: Vec<String> = store.list().iter().map(|job| job.name.clone()).collect();
    names.sort();
    names
}

#[test]
fn cron_store_parse_error_keeps_user_jobs_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("crons.json");
    let mut store = CronStore::new_with_path(path.clone());
    store.create(params("havoc-far")).unwrap();
    store.create(params("havoc-far2")).unwrap();
    let full = std::fs::read(&path).unwrap();
    let truncated = full[..full.len() / 2].to_vec();
    std::fs::write(&path, &truncated).unwrap();

    let recovered = CronStore::new_with_path(path.clone());

    assert_eq!(names(&recovered), vec!["havoc-far", "havoc-far2"]);
    let copies = state_file(&path).corrupt_copies();
    assert_eq!(copies.len(), 1);
    assert_eq!(std::fs::read(&copies[0]).unwrap(), truncated);
    assert_eq!(
        std::fs::read(&path).unwrap(),
        full,
        "restored from the newest backup"
    );
    assert_eq!(crate::persisted_state::incidents_for(&path).len(), 1);
}

#[test]
fn cron_store_parse_error_without_backup_never_persists_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("crons.json");
    let broken = br#"[{"id": "broken", "name": "#.to_vec();
    std::fs::write(&path, &broken).unwrap();

    let mut store = CronStore::new_with_path(path.clone());

    assert!(store.list().is_empty());
    assert!(!path.exists());
    let built_in = store.create(params("mem-dreaming-auto"));
    assert!(
        built_in.is_err(),
        "the save is refused while the file is held"
    );
    assert_eq!(
        names(&store),
        vec!["mem-dreaming-auto"],
        "merged in memory only"
    );
    assert!(store.save().is_err());
    assert!(
        !path.exists(),
        "defaults are never persisted over the corrupt file"
    );
    let copies = state_file(&path).corrupt_copies();
    assert_eq!(copies.len(), 1);
    assert_eq!(std::fs::read(&copies[0]).unwrap(), broken);
}
