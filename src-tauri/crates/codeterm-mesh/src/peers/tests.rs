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
fn corrupt_revocation_list_keeps_revoked_peers() {
    let dir = TempDir::new();
    let path = dir.0.join("revocation.json");
    let list = RevocationList {
        revoked: vec!["peer-a".into()],
        ..RevocationList::default()
    };
    save_revocation_list_at(&path, &list).unwrap();
    std::fs::write(&path, "{\"revoked\": [").unwrap();

    assert_eq!(load_revocation_list_at(&path).revoked, vec!["peer-a".to_string()]);

    std::fs::remove_dir_all(revocation_file(&path).backup_dir()).unwrap();
    std::fs::write(&path, "{\"revoked\": [").unwrap();
    let held = load_revocation_list_at(&path);
    assert!(held.revoked.is_empty());
    assert!(save_revocation_list_at(&path, &held).is_err());
    assert!(!path.exists(), "an empty list is never saved over the revoked peers");
}

#[test]
fn corrupt_peers_file_is_held_not_emptied() {
    let dir = TempDir::new();
    let path = dir.0.join("peers.json");
    let broken = b"{\"peers\": {\"x\": ".to_vec();
    std::fs::write(&path, &broken).unwrap();

    let peers = load_peers_at(&path);

    assert!(peers.is_empty());
    assert!(save_peers_at(&path, &peers).is_err());
    assert!(!path.exists());
    let copies = peers_file(&path).corrupt_copies();
    assert_eq!(std::fs::read(&copies[0]).unwrap(), broken);
}
