use super::*;
use crate::types::PairingStatus;

const PEER_ID: &str = "2895951d-328b-4f9a-8db5-9271b4353b6e";

fn manager(name: &str, port: u16) -> MeshManager {
    let (private, public) = crate::crown::generate_peer_keypair();
    MeshManager::in_memory(
        crate::types::MeshIdentity {
            instance_id: uuid::Uuid::new_v4().to_string(),
            private_key: Some(private),
            public_key: Some(public),
        },
        name.into(),
        port,
    )
}

fn fixture() -> (IdentityProof, IdentityChallenge) {
    let (private, public) = crate::crown::generate_peer_keypair();
    let challenge = IdentityChallenge {
        address: "100.64.10.1:7686".parse().unwrap(),
        nonce: uuid::Uuid::new_v4(),
    };
    let identity = PeerIdentity {
        challenge: challenge.clone(),
        instance_id: PEER_ID.into(),
        instance_name: "Windows [dev]".into(),
        public_key: public,
        dev: true,
        addresses: vec![PeerAddress::from_ip("100.64.10.1")],
        api_port: 7686,
        mesh_id: None,
    };
    (IdentityProof::sign(identity, &private).unwrap(), challenge)
}

#[test]
fn mesh_manual_registration_uses_shared_merge_without_pairing() {
    let (proof, challenge) = fixture();
    let mut mesh = manager("local", 7697);
    mesh.upsert_discovered(
        DiscoverySighting {
            instance_id: PEER_ID.into(),
            instance_name: "old".into(),
            addresses: vec![PeerAddress::from_ip("192.168.1.9")],
            api_port: 7697,
            method: DiscoveryMethod::Gossip,
            dev: false,
            mesh_id: Some("mesh".into()),
            voucher: Some("voucher".into()),
        },
        10,
    );
    let id = register_identity(&mut mesh, &proof, &challenge, Some("2895951d"), 20).unwrap();
    let peer = &mesh.discovered_peers[&id];
    assert_eq!(peer.discovery_method, DiscoveryMethod::Manual);
    assert_eq!(peer.api_port, 7686);
    assert_eq!(peer.last_seen, 20);
    assert_eq!(peer.addresses[0].ip, "100.64.10.1");
    assert_eq!(peer.addresses.len(), 2);
    assert_eq!(peer.vouched_by, vec!["voucher"]);
    assert!(peer.dev);
    assert!(mesh.paired_peers.is_empty());
    assert!(mesh
        .pending_requests
        .values()
        .all(|r| r.status != PairingStatus::Accepted));
}

#[test]
fn mesh_manual_id_mismatch_refused_before_registration() {
    let (proof, challenge) = fixture();
    let mut mesh = manager("local", 7697);
    for target in ["aaaaaaaa-328b-4f9a-8db5-9271b4353b6e", "aaaaaaaa", "Other"] {
        assert!(
            register_identity(&mut mesh, &proof, &challenge, Some(target), 20)
                .unwrap_err()
                .starts_with("mesh_identity_mismatch")
        );
        assert!(mesh.discovered_peers.is_empty());
        assert!(mesh.pending_requests.is_empty());
    }
    assert!(proof.verify(&challenge, Some("Windows [dev]")).is_ok());
}

#[test]
fn mesh_manual_proof_binds_address_nonce_and_signed_identity() {
    let (proof, challenge) = fixture();
    let mut wrong = challenge.clone();
    wrong.address.set_port(7697);
    assert_eq!(
        proof.verify(&wrong, Some(PEER_ID)).err().as_deref(),
        Some("mesh_identity_challenge_mismatch")
    );
    wrong = challenge.clone();
    wrong.nonce = uuid::Uuid::new_v4();
    assert!(proof.verify(&wrong, Some(PEER_ID)).is_err());
    let mut tampered = proof.clone();
    tampered.identity.instance_name = "tampered".into();
    assert_eq!(
        tampered.verify(&challenge, Some(PEER_ID)).err().as_deref(),
        Some("mesh_identity_signature_invalid")
    );
}

#[test]
fn mesh_manual_registration_preserves_revocation_block_and_self_refusal() {
    let (proof, challenge) = fixture();
    let mut mesh = manager("local", 7697);
    mesh.ident.identity.instance_id = PEER_ID.into();
    assert!(register_identity(&mut mesh, &proof, &challenge, Some(PEER_ID), 20).is_err());
    mesh.ident.identity.instance_id = "local".into();
    mesh.revocation_list.revoked.push(PEER_ID.into());
    assert!(register_identity(&mut mesh, &proof, &challenge, Some(PEER_ID), 20).is_err());
    mesh.revocation_list.revoked.clear();
    mesh.revocation_list.blocked.push(PEER_ID.into());
    assert!(register_identity(&mut mesh, &proof, &challenge, Some(PEER_ID), 20).is_err());
    assert!(mesh.discovered_peers.is_empty());
}

#[test]
fn mesh_manual_identity_snapshot_contains_only_ready_listener_addresses() {
    let mut mesh = manager("fixture [dev]", 7686);
    let (private, public) = crate::crown::generate_peer_keypair();
    mesh.ident.identity.private_key = Some(private);
    mesh.ident.identity.public_key = Some(public.clone());
    mesh.local_ip = Some("192.168.1.8".into());
    mesh.ready_listeners = vec![
        "127.0.0.1:7686".parse().unwrap(),
        "100.64.10.1:7686".parse().unwrap(),
    ];
    let challenge = IdentityChallenge {
        address: "100.64.10.1:7686".parse().unwrap(),
        nonce: uuid::Uuid::new_v4(),
    };
    let proof = IdentityProof::from_manager(&mesh, challenge.clone()).unwrap();
    assert_eq!(proof.identity.public_key, public);
    assert_eq!(proof.identity.addresses.len(), 1);
    assert_eq!(proof.identity.addresses[0].ip, "100.64.10.1");
    assert!(proof
        .verify(&challenge, Some(&mesh.ident.identity.instance_id))
        .is_ok());
    let mut wrong = challenge.clone();
    wrong.address = "192.168.1.8:7686".parse().unwrap();
    assert!(IdentityProof::from_manager(&mesh, wrong).is_err());
    mesh.ready_listeners.clear();
    assert!(IdentityProof::from_manager(&mesh, challenge).is_err());
}
