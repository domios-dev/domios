use super::*;

#[test]
fn generated_credential_survives_restart_without_rotation() {
    let root = tempfile::tempdir().unwrap();
    let first = initialize_at(root.path(), None, true).unwrap().unwrap();
    assert_eq!(first.len(), 32);
    assert_eq!(initialize_at(root.path(), None, true).unwrap(), Some(first));
    assert!(crate::private_file::owner_only(&root.path().join(".secrets")).unwrap());
}

#[test]
fn initialization_preserves_existing_authority_and_other_services() {
    let root = tempfile::tempdir().unwrap();
    SecretFile::new_at(root.path(), "other")
        .set("key", "other-value")
        .unwrap();
    assert_eq!(
        initialize_at(root.path(), Some("existing-bearer"), false)
            .unwrap()
            .as_deref(),
        Some("existing-bearer")
    );
    assert_eq!(
        initialize_at(root.path(), Some("stale-config-bearer"), false)
            .unwrap()
            .as_deref(),
        Some("existing-bearer")
    );
    assert_eq!(
        SecretFile::new_at(root.path(), "other").get("key").unwrap(),
        "other-value"
    );
}

#[test]
fn explicit_dev_identity_never_falls_back_to_inherited_production_token() {
    let prod = tempfile::tempdir().unwrap();
    let dev = tempfile::tempdir().unwrap();
    initialize_at(prod.path(), Some("production"), false).unwrap();
    assert_eq!(
        resolve_at(dev.path(), Some("production"), true).unwrap(),
        None
    );
    initialize_at(dev.path(), Some("development"), false).unwrap();
    assert_eq!(
        resolve_at(dev.path(), Some("production"), true)
            .unwrap()
            .as_deref(),
        Some("development")
    );
    assert_eq!(
        resolve_at(prod.path(), Some("scoped-environment"), false)
            .unwrap()
            .as_deref(),
        Some("scoped-environment")
    );
}

#[test]
fn damaged_store_is_preserved_and_generation_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(".secrets");
    std::fs::write(&path, "damaged credential store").unwrap();
    assert!(initialize_at(root.path(), Some("legacy"), true).is_err());
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "damaged credential store"
    );
}

#[test]
fn held_config_without_credential_does_not_generate_one() {
    let root = tempfile::tempdir().unwrap();
    assert_eq!(initialize_at(root.path(), None, false).unwrap(), None);
    assert!(!root.path().join(".secrets").exists());
}

#[test]
fn migration_rotates_once_and_invalidates_the_legacy_bearer() {
    let root = tempfile::tempdir().unwrap();
    initialize_at(root.path(), Some("legacy-bearer"), false).unwrap();
    let rotated = migrate_at(root.path(), "legacy-bearer").unwrap();
    assert_ne!(rotated, "legacy-bearer");
    assert_eq!(
        read_at(root.path()).unwrap().as_deref(),
        Some(rotated.as_str())
    );
    assert_eq!(migrate_at(root.path(), "legacy-bearer").unwrap(), rotated);
}

#[test]
fn failed_migration_preserves_the_damaged_store() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(".secrets");
    std::fs::write(&path, "damaged").unwrap();
    assert!(migrate_at(root.path(), "legacy-bearer").is_err());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "damaged");
}
