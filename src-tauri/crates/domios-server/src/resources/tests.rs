use super::*;

#[test]
fn process_split_contract_worker_resource_rejects_missing_and_wrong_artifacts() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("worker-resource");
    assert!(verify_worker_cli(&path, &"0".repeat(64), false).is_err());
    std::fs::write(&path, b"worker-fixture").unwrap();
    let expected = format!("{:x}", Sha256::digest(b"worker-fixture"));
    assert!(verify_worker_cli(&path, &expected, false).is_ok());
    assert!(verify_worker_cli(&path, &"0".repeat(64), false).is_err());
}

#[test]
fn process_split_contract_worker_lease_survives_packaged_resource_replacement() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("packaged-cli");
    std::fs::write(&source, b"original-worker").unwrap();
    let expected = format!("{:x}", Sha256::digest(b"original-worker"));
    let lease = verify_worker_cli(&source, &expected, false).unwrap();
    assert_ne!(lease.path, source);
    std::fs::write(&source, b"replacement").unwrap();
    assert_eq!(std::fs::read(&lease.path).unwrap(), b"original-worker");
    let destination = lease.path.clone();
    let alias = destination
        .parent()
        .unwrap()
        .join(format!("domios{}", std::env::consts::EXE_SUFFIX));
    assert_eq!(std::fs::read(&alias).unwrap(), b"original-worker");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(
            std::fs::metadata(&alias).unwrap().permissions().mode() & 0o100,
            0
        );
    }
    drop(lease);
    assert!(!destination.exists());
    assert!(!alias.exists());
}

#[cfg(unix)]
#[test]
fn cloud_worker_lease_uses_the_verified_bytes_for_both_names_and_refuses_corruption() {
    use std::os::unix::fs::MetadataExt;

    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("packaged-cli");
    let original = b"original-worker";
    std::fs::write(&source, original).unwrap();
    let expected = format!("{:x}", Sha256::digest(original));
    let lease = verify_worker_cli(&source, &expected, true).unwrap();
    let alias = lease.path.with_file_name("domios");
    let primary = std::fs::metadata(&lease.path).unwrap();
    let alias_metadata = std::fs::symlink_metadata(&alias).unwrap();
    assert_eq!(
        (primary.dev(), primary.ino()),
        (alias_metadata.dev(), alias_metadata.ino())
    );
    assert_eq!(primary.mode() & 0o777, 0o500);

    let mut corrupt = *original;
    corrupt[0] ^= 1;
    std::fs::write(&source, corrupt).unwrap();
    assert!(verify_worker_cli(&source, &expected, true).is_err());
    assert_eq!(std::fs::read(&lease.path).unwrap(), original);
    assert_eq!(std::fs::read(alias).unwrap(), original);
}
