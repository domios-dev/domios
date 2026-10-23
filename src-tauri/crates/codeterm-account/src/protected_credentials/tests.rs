use super::*;

#[test]
fn migration_decision_is_local_and_once() {
    assert_eq!(migration_decision(false, false), MigrationDecision::Claim);
    for (present, attempted) in [(true, false), (false, true), (true, true)] {
        assert_eq!(
            migration_decision(present, attempted),
            MigrationDecision::Skip
        );
    }
}

#[test]
fn private_file_round_trip_preserves_instance_token_and_permissions() {
    let root = tempfile::tempdir().unwrap();
    let api_token = codeterm_util::instance_token::initialize_at(root.path(), None, true).unwrap();
    let store = ProtectedCredentials::new(root.path());
    store
        .save("refresh", &Secret::new("opaque-refresh"))
        .unwrap();
    store.save("mesh", &Secret::new("opaque-mesh")).unwrap();
    let reopened = ProtectedCredentials::new(root.path());
    assert_eq!(
        reopened.load("refresh").unwrap().unwrap().expose(),
        "opaque-refresh"
    );
    assert_eq!(
        reopened.load("mesh").unwrap().unwrap().expose(),
        "opaque-mesh"
    );
    assert_eq!(
        codeterm_util::instance_token::read_at(root.path()).unwrap(),
        api_token
    );
    assert!(codeterm_util::private_file::owner_only(&root.path().join(".secrets")).unwrap());
    assert!(!store.session_only("refresh"));
}

#[test]
fn unavailable_private_file_keeps_refresh_credential_in_memory_only() {
    let root = tempfile::tempdir().unwrap();
    let bad_root = root.path().join("file");
    std::fs::write(&bad_root, "occupied").unwrap();
    let store = ProtectedCredentials::new(bad_root);
    store.save("refresh", &Secret::new("opaque")).unwrap();
    assert!(store.session_only("refresh"));
    assert_eq!(store.load("refresh").unwrap().unwrap().expose(), "opaque");
    store.delete("refresh").unwrap();
    assert!(!store.session_only("refresh"));
    assert!(store.load("refresh").unwrap().is_none());
}

#[test]
fn migration_attempt_survives_restart_and_never_retries() {
    let root = tempfile::tempdir().unwrap();
    let store = ProtectedCredentials::new(root.path());
    assert!(store.claim_migration("refresh").unwrap());
    assert!(!store.claim_migration("refresh").unwrap());
    let reopened = ProtectedCredentials::new(root.path());
    assert!(!reopened.claim_migration("refresh").unwrap());
}

#[test]
fn newer_save_and_logout_cancel_inflight_migration() {
    let root = tempfile::tempdir().unwrap();
    let store = ProtectedCredentials::new(root.path());
    assert!(store.claim_migration("refresh").unwrap());
    store.save("refresh", &Secret::new("new")).unwrap();
    assert!(!store
        .finish_migration("refresh", &Secret::new("old"))
        .unwrap());
    assert_eq!(store.load("refresh").unwrap().unwrap().expose(), "new");
    assert!(store.claim_migration("mesh").unwrap());
    store.delete("mesh").unwrap();
    assert!(!store.finish_migration("mesh", &Secret::new("old")).unwrap());
    assert!(store.load("mesh").unwrap().is_none());
    assert!(!ProtectedCredentials::new(root.path())
        .claim_migration("mesh")
        .unwrap());
}

#[test]
fn failed_migration_write_retains_old_copy() {
    let root = tempfile::tempdir().unwrap();
    let store = ProtectedCredentials::new(root.path());
    assert!(store.claim_migration("refresh").unwrap());
    std::fs::remove_file(root.path().join(".secrets")).unwrap();
    std::fs::create_dir(root.path().join(".secrets")).unwrap();
    assert!(!store
        .finish_migration("refresh", &Secret::new("old"))
        .unwrap());
    assert!(store.session_only("refresh"));
}
