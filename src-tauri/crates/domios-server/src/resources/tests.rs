use super::*;

#[test]
fn process_split_contract_worker_resource_rejects_missing_and_wrong_artifacts() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("worker-resource");
    assert!(verify_worker_cli(&path, &"0".repeat(64)).is_err());
    std::fs::write(&path, b"worker-fixture").unwrap();
    let expected = format!("{:x}", Sha256::digest(b"worker-fixture"));
    assert!(verify_worker_cli(&path, &expected).is_ok());
    assert!(verify_worker_cli(&path, &"0".repeat(64)).is_err());
}

#[test]
fn process_split_contract_worker_lease_survives_packaged_resource_replacement() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("packaged-cli");
    std::fs::write(&source, b"original-worker").unwrap();
    let expected = format!("{:x}", Sha256::digest(b"original-worker"));
    let lease = verify_worker_cli(&source, &expected).unwrap();
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
