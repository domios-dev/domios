use super::*;

#[test]
fn generated_credential_survives_restart_without_rotation() {
    let root = tempfile::tempdir().unwrap();
    let first = initialize_at(root.path(), true).unwrap().unwrap();
    assert_eq!(first.len(), 32);
    assert_eq!(initialize_at(root.path(), true).unwrap(), Some(first));
    assert!(crate::private_file::owner_only(&root.path().join(".secrets")).unwrap());
}

#[test]
fn initialization_preserves_existing_authority_and_other_services() {
    let root = tempfile::tempdir().unwrap();
    SecretFile::new_at(root.path(), "other")
        .set("key", "other-value")
        .unwrap();
    store(root.path()).unwrap().set(ACCOUNT, "existing-bearer").unwrap();
    assert_eq!(
        initialize_at(root.path(), false)
            .unwrap()
            .as_deref(),
        Some("existing-bearer")
    );
    assert_eq!(
        initialize_at(root.path(), false)
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
    store(prod.path()).unwrap().set(ACCOUNT, "production").unwrap();
    assert_eq!(
        resolve_at(dev.path(), Some("production"), true).unwrap(),
        None
    );
    store(dev.path()).unwrap().set(ACCOUNT, "development").unwrap();
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
    assert!(initialize_at(root.path(), true).is_err());
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "damaged credential store"
    );
}

#[test]
fn held_config_without_credential_does_not_generate_one() {
    let root = tempfile::tempdir().unwrap();
    assert_eq!(initialize_at(root.path(), false).unwrap(), None);
    assert!(!root.path().join(".secrets").exists());
}

