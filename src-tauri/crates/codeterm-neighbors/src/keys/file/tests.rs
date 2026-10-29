use super::*;

#[cfg(target_os = "linux")]
#[tokio::test]
async fn round_trip_is_private_and_permission_changes_refuse_all_access() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let data = tempfile::tempdir().unwrap();
    let store = FileKeyStore::new(data.path().into());
    let scope = KeyScope::installation("test-instance", "owner").unwrap();
    let bytes = Zeroizing::new(vec![7; 64]);
    store
        .save(&scope, Slot::Current, bytes.clone())
        .await
        .unwrap();
    let path = store.root.join(scope.name(Slot::Current));
    assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o7777, 0o600);
    assert_eq!(
        store.load(&scope, Slot::Current).await.unwrap().unwrap(),
        bytes
    );
    for mode in [0o644, 0o640] {
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        assert!(store.load(&scope, Slot::Current).await.is_err());
        assert!(store
            .save(&scope, Slot::Current, Zeroizing::new(vec![8; 64]))
            .await
            .is_err());
        assert!(store.delete(&scope, Slot::Current).await.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), vec![7; 64]);
    }
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn symlinks_and_hardlinks_are_refused() {
    let data = tempfile::tempdir().unwrap();
    let store = FileKeyStore::new(data.path().into());
    let scope = KeyScope::installation("test-instance", "owner").unwrap();
    store
        .save(&scope, Slot::Current, Zeroizing::new(vec![7; 64]))
        .await
        .unwrap();
    let original = store.root.join(scope.name(Slot::Current));
    let candidate = store.root.join(scope.name(Slot::Candidate));
    std::os::unix::fs::symlink(&original, &candidate).unwrap();
    assert!(store.load(&scope, Slot::Candidate).await.is_err());
    std::fs::remove_file(&candidate).unwrap();
    std::fs::hard_link(&original, &candidate).unwrap();
    assert!(store
        .save(&scope, Slot::Current, Zeroizing::new(vec![8; 64]))
        .await
        .is_err());
    assert_eq!(std::fs::read(original).unwrap(), vec![7; 64]);
}

#[cfg(not(target_os = "linux"))]
#[tokio::test]
async fn file_store_is_refused_off_linux() {
    let data = tempfile::tempdir().unwrap();
    let store = FileKeyStore::new(data.path().into());
    let scope = KeyScope::installation("test-instance", "owner").unwrap();
    assert!(store.load(&scope, Slot::Current).await.is_err());
    assert!(store
        .save(&scope, Slot::Current, Zeroizing::new(vec![7; 64]))
        .await
        .is_err());
    assert!(store.delete(&scope, Slot::Current).await.is_err());
    assert!(!store.root.exists());
}
