use super::*;

#[test]
fn local_identity_needs_persisted_credentials_and_invalidation_reaches_clones() {
    let root = tempfile::tempdir().unwrap();
    let identity = Identity {
        uid: "opaque-owner".into(),
        email: None,
        display_name: None,
        provider: "fixture".into(),
        photo_url: None,
    };
    remember(root.path(), "fixture", Some(identity.clone())).unwrap();
    assert!(load(root.path()).is_none());
    let file = SecretFile::new_at(root.path(), "domios.account");
    let credential = super::super::credentials::credential_name("fixture");
    file.set(&credential, "opaque-refresh").unwrap();
    remember(root.path(), "fixture", Some(identity.clone())).unwrap();
    let cached = load(root.path()).unwrap();
    let clone = cached.clone();
    assert_eq!(cached.status().identity, Some(identity));
    let snapshot = cached.snapshot().unwrap();
    assert!(snapshot.checking);
    assert!(snapshot.access.is_none());
    assert!(!cached.gate().summary().enforcing);
    cached.invalidate();
    assert!(clone.snapshot().is_none());
    remember(root.path(), "fixture", None).unwrap();
    assert!(load(root.path()).is_none());
}
