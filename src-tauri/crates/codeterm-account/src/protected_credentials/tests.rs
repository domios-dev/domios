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
fn pending_server_stop_requires_persistent_private_storage() {
    let root = tempfile::tempdir().unwrap();
    let bad_root = root.path().join("file");
    std::fs::write(&bad_root, "occupied").unwrap();
    let store = ProtectedCredentials::new(bad_root);
    assert_eq!(
        store.save_persistent("stop", &Secret::new("id-token")),
        Err(Error::Credentials)
    );
    assert!(store.load("stop").unwrap().is_none());
    let store = ProtectedCredentials::new(root.path());
    store
        .save_persistent("stop", &Secret::new("id-token"))
        .unwrap();
    assert_eq!(
        ProtectedCredentials::new(root.path())
            .load("stop")
            .unwrap()
            .unwrap()
            .expose(),
        "id-token"
    );
}

#[test]
fn interrupted_migration_retries_after_restart() {
    let root = tempfile::tempdir().unwrap();
    let store = ProtectedCredentials::new(root.path());
    assert!(store.claim_migration("refresh").unwrap());
    assert!(!store.claim_migration("refresh").unwrap());
    let reopened = ProtectedCredentials::new(root.path());
    assert!(reopened.claim_migration("refresh").unwrap());
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
    std::fs::create_dir(root.path().join(".secrets")).unwrap();
    assert!(!store
        .finish_migration("refresh", &Secret::new("old"))
        .unwrap());
    assert!(store.session_only("refresh"));
}

#[test]
fn migration_completion_requires_a_definitive_vault_result() {
    assert_eq!(
        migration_completion(&Ok(Some(Secret::new("found")))),
        MigrationCompletion::Persist
    );
    assert_eq!(
        migration_completion(&Ok(None)),
        MigrationCompletion::MarkAbsent
    );
    for error in [Error::Credentials, Error::Timeout] {
        assert_eq!(
            migration_completion(&Err(error)),
            MigrationCompletion::Retry
        );
    }
}

#[test]
fn failed_vault_read_retries_and_absence_completes() {
    let root = tempfile::tempdir().unwrap();
    let store = ProtectedCredentials::new(root.path());
    assert!(store.claim_migration("refresh").unwrap());
    assert!(!store
        .complete_migration("refresh", Err(Error::Credentials))
        .unwrap());
    assert!(store.claim_migration("refresh").unwrap());
    assert!(!store.complete_migration("refresh", Ok(None)).unwrap());
    assert!(!ProtectedCredentials::new(root.path())
        .claim_migration("refresh")
        .unwrap());
}

#[test]
fn migrated_token_survives_interruption_before_legacy_cleanup() {
    let root = tempfile::tempdir().unwrap();
    let store = ProtectedCredentials::new(root.path());
    assert!(store.claim_migration("refresh").unwrap());
    assert!(store
        .complete_migration("refresh", Ok(Some(Secret::new("found"))))
        .unwrap());
    let reopened = ProtectedCredentials::new(root.path());
    assert!(!reopened.claim_migration("refresh").unwrap());
    assert_eq!(reopened.load("refresh").unwrap().unwrap().expose(), "found");
}
