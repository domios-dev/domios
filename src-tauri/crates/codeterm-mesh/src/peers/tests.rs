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

    assert_eq!(
        load_revocation_list_at(&path).revoked,
        vec!["peer-a".to_string()]
    );

    std::fs::remove_dir_all(revocation_file(&path).backup_dir()).unwrap();
    std::fs::write(&path, "{\"revoked\": [").unwrap();
    let held = load_revocation_list_at(&path);
    assert!(held.revoked.is_empty());
    assert!(save_revocation_list_at(&path, &held).is_err());
    assert!(
        !path.exists(),
        "an empty list is never saved over the revoked peers"
    );
}

#[test]
fn corrupt_peers_file_is_held_not_emptied() {
    let dir = TempDir::new();
    let path = dir.0.join("peers.json");
    let broken = b"{\"peers\": {\"x\": ".to_vec();
    std::fs::write(&path, &broken).unwrap();

    let peers = load_peers_at(&path);

    assert!(peers.is_empty());
    assert!(save_peers_at(&path, &peers, &LinkLedger::default()).is_err());
    assert!(!path.exists());
    let copies = peers_file(&path).corrupt_copies();
    assert_eq!(std::fs::read(&copies[0]).unwrap(), broken);
}

#[test]
fn unreadable_peers_file_does_not_overwrite_link_ledger() {
    let dir = TempDir::new();
    let path = dir.0.join("peers.json");
    let ledger_path = account_mesh_ledger_path(&path);
    let original = LinkLedger {
        owner_account: Some("account-a".into()),
        ..LinkLedger::default()
    };
    std::fs::write(&ledger_path, serde_json::to_vec(&original).unwrap()).unwrap();
    let original_bytes = std::fs::read(&ledger_path).unwrap();
    std::fs::write(&path, b"{ unreadable").unwrap();
    assert!(load_peers_at(&path).is_empty());

    assert!(save_peers_at(&path, &HashMap::new(), &LinkLedger::default()).is_err());
    assert_eq!(std::fs::read(&ledger_path).unwrap(), original_bytes);
}

#[test]
fn account_mesh_ledger_survives_an_old_peers_writer() {
    use crate::account_mesh::ledger::{LinkAdmission, LinkRecord};

    let dir = TempDir::new();
    let path = dir.0.join("peers.json");
    let peer_id = "peer-a";
    let payload = serde_json::json!({
        "peers": {
            peer_id: {
                "instance_id": peer_id,
                "instance_name": "old-peer",
                "addresses": [],
                "api_port": 7685,
                "public_key": "peer-key",
                "paired_at": "2026-01-01T00:00:00Z",
                "last_seen": null,
                "version": null,
                "relay_protocol_version": 1
            }
        }
    });
    let (peers, _) = rehydrate_peers_payload(&payload.to_string()).unwrap();
    let ledger = LinkLedger {
        owner_account: Some("account-a".into()),
        links: [(
            peer_id.to_string(),
            LinkRecord {
                admission: LinkAdmission::Adopted {
                    account_id: "account-a".into(),
                    installation_id: "installation-a".into(),
                    public_key: "peer-key".into(),
                },
                remote_membership: None,
            },
        )]
        .into_iter()
        .collect(),
    };

    save_peers_at(&path, &peers, &ledger).unwrap();
    let mut old_writer_payload: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    old_writer_payload
        .as_object_mut()
        .unwrap()
        .remove("account_mesh");
    std::fs::write(&path, old_writer_payload.to_string()).unwrap();

    let restored = load_mesh_records_at(&path)
        .ledger
        .expect("the independent ledger survives a peers-only rewrite");
    assert_eq!(restored.owner_account.as_deref(), Some("account-a"));
    assert!(matches!(
        restored.get(peer_id).map(|record| &record.admission),
        Some(LinkAdmission::Adopted { account_id, .. }) if account_id == "account-a"
    ));
    let mut restored = restored;
    assert!(!restored.bind_owner("account-b"));
    assert_eq!(restored.owner_account.as_deref(), Some("account-a"));
}
