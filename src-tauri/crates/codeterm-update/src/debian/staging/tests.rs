use super::*;

#[test]
fn failure_is_consumed_by_the_next_start() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("debian-update-failure.txt"), "failed").unwrap();
    assert_eq!(failure(root.path()), Some("failed".into()));
    assert_eq!(failure(root.path()), None);
}

#[test]
fn rollback_without_a_receipt_requires_unowned_instance_paths() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("instance.lock");
    let owner = File::create(&path).unwrap();
    owner.lock().unwrap();
    assert!(instance_unused(std::slice::from_ref(&path)).is_err());
    drop(owner);
    assert!(instance_unused(std::slice::from_ref(&path)).is_ok());
    assert!(instance_unused(&[]).is_err());
    std::fs::remove_file(&path).unwrap();
    assert!(instance_unused(&[path]).is_err());
}

#[test]
fn cleanup_requires_owned_unused_and_stale() {
    let stale = Duration::from_secs(86400);
    assert!(cleanup_selected(true, false, stale));
    assert!(!cleanup_selected(false, false, stale));
    assert!(!cleanup_selected(true, true, stale));
    assert!(!cleanup_selected(true, false, Duration::ZERO));
}

#[test]
fn lease_excludes_other_installers_and_cleanup() {
    let mut directory = tempfile::Builder::new();
    directory.prefix(PREFIX);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        directory.permissions(std::fs::Permissions::from_mode(0o700));
    }
    let root = directory.tempdir().unwrap();
    let first = StageLease::acquire(root.path()).unwrap();
    assert!(StageLease::acquire(root.path()).is_err());
    drop(first);
    assert!(StageLease::acquire(root.path()).is_ok());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o770)).unwrap();
        assert!(!owned(root.path()));
        assert!(StageLease::acquire(root.path()).is_err());
    }
}

#[test]
fn foreign_and_symlink_directories_are_never_owned() {
    let root = tempfile::tempdir().unwrap();
    assert!(!owned(root.path()));
    #[cfg(unix)]
    {
        let link = root.path().join(format!("{PREFIX}link"));
        std::os::unix::fs::symlink(root.path(), &link).unwrap();
        assert!(!owned(&link));
    }
}
