use super::*;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("mesh-peers-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn identity_is_stable_across_loads() {
    let dir = TempDir::new();
    let path = dir.0.join("identity.json");
    let id1 = load_or_create_identity_at(&path);
    let id2 = load_or_create_identity_at(&path);
    assert_eq!(id1.instance_id, id2.instance_id);
    assert_eq!(id1.private_key, id2.private_key);
}

#[test]
fn corrupt_identity_recovers_the_same_keypair_from_backup() {
    let dir = TempDir::new();
    let path = dir.0.join("identity.json");
    let original = load_or_create_identity_at(&path);
    let broken = b"{\"instance_id\": \"trunc".to_vec();
    std::fs::write(&path, &broken).unwrap();

    let recovered = load_or_create_identity_at(&path);

    assert_eq!(recovered.instance_id, original.instance_id);
    assert_eq!(recovered.private_key, original.private_key);
    let copies = identity_file(&path).corrupt_copies();
    assert_eq!(std::fs::read(&copies[0]).unwrap(), broken);
}

#[test]
fn corrupt_identity_without_backup_is_never_replaced_on_disk() {
    let dir = TempDir::new();
    let path = dir.0.join("identity.json");
    let broken = b"{\"instance_id\": ".to_vec();
    std::fs::write(&path, &broken).unwrap();

    let temporary = load_or_create_identity_at(&path);
    save_identity(&temporary);

    assert!(!path.exists());
    let copies = identity_file(&path).corrupt_copies();
    assert_eq!(copies.len(), 1);
    assert_eq!(std::fs::read(&copies[0]).unwrap(), broken);
}

#[test]
fn legacy_pair_store_is_removed_without_reading_or_touching_other_state() {
    let directory = TempDir::new();
    for name in [
        "identity.json",
        "revocation.json",
        "account.json",
        "user.json",
    ] {
        std::fs::write(directory.0.join(name), name).unwrap();
    }
    let pair = directory.0.join("peers.json");
    let ledger = directory.0.join("account_mesh_ledger.json");
    for path in [&pair, &ledger] {
        std::fs::write(path, "corrupt pair data").unwrap();
        let file = StateFile::new(path);
        std::fs::create_dir_all(file.backup_dir()).unwrap();
        std::fs::write(file.backup_dir().join("old.bak"), "old pair data").unwrap();
        std::fs::write(
            path.with_file_name(format!(
                "{}.corrupt-1",
                path.file_name().unwrap().to_string_lossy()
            )),
            "corrupt pair data",
        )
        .unwrap();
    }
    let result = remove_legacy_pair_store_at(&directory.0);
    assert!(result.failed.is_empty());
    assert_eq!(result.removed.len(), 6);
    for path in &result.removed {
        assert!(!path.exists());
    }
    for name in [
        "identity.json",
        "revocation.json",
        "account.json",
        "user.json",
    ] {
        assert_eq!(
            std::fs::read_to_string(directory.0.join(name)).unwrap(),
            name
        );
    }
    assert!(remove_legacy_pair_store_at(&directory.0).removed.is_empty());
}
