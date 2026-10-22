use super::*;
use tempfile::tempdir;

#[test]
fn generate_persist_validate_round_trip() {
    let dir = tempdir().unwrap();
    let store = TokenStore::new_with_root(dir.path().to_path_buf());
    let token = store.generate("svc_test").unwrap();
    assert_eq!(token.len(), 64);
    assert!(codeterm_util::private_file::owner_only(&store.token_path("svc_test")).unwrap());
    assert!(store.validate("svc_test", &token).unwrap());
    assert!(!store.validate("svc_test", "wrong-token").unwrap());
}

#[test]
fn missing_service_returns_none() {
    let dir = tempdir().unwrap();
    let store = TokenStore::new_with_root(dir.path().to_path_buf());
    assert!(!store.validate("svc_nonexistent", "any").unwrap());
}

#[test]
fn rotate_invalidates_old_token() {
    let dir = tempdir().unwrap();
    let store = TokenStore::new_with_root(dir.path().to_path_buf());
    let old = store.generate("svc_x").unwrap();
    let new = store.generate("svc_x").unwrap();
    assert_ne!(old, new);
    assert!(!store.validate("svc_x", &old).unwrap());
    assert!(store.validate("svc_x", &new).unwrap());
}

#[test]
fn lookup_service_id_by_token() {
    let dir = tempdir().unwrap();
    let store = TokenStore::new_with_root(dir.path().to_path_buf());
    let token_a = store.generate("svc_a").unwrap();
    let token_b = store.generate("svc_b").unwrap();
    assert_eq!(
        store.lookup_service_id(&token_a).unwrap(),
        Some("svc_a".to_string())
    );
    assert_eq!(
        store.lookup_service_id(&token_b).unwrap(),
        Some("svc_b".to_string())
    );
    assert_eq!(store.lookup_service_id("garbage").unwrap(), None);
}

#[test]
fn delete_removes_token_dir() {
    let dir = tempdir().unwrap();
    let store = TokenStore::new_with_root(dir.path().to_path_buf());
    let _ = store.generate("svc_to_delete").unwrap();
    store.delete("svc_to_delete").unwrap();
    assert!(!store.validate("svc_to_delete", "any").unwrap());
}
