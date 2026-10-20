use super::*;
use crate::types::{AccessLevel, DiscoveredPeer, DiscoveryMethod, PairedPeer, PeerAddress};

const REMOTE_ID: &str = "31c7cff6-4bd2-43b0-a5cd-b5db9bfbb673";
const PLACEHOLDER: &str = "manual-placeholder";

fn discovered_peer(id: &str, method: DiscoveryMethod) -> DiscoveredPeer {
    DiscoveredPeer {
        instance_id: id.into(),
        instance_name: "peer".into(),
        addresses: vec![PeerAddress::from_ip("127.0.0.1")],
        api_port: 7786,
        discovery_method: method,
        last_seen: 0,
        dev: true,
        mesh_id: None,
        vouched_by: Vec::new(),
    }
}

fn paired_peer(id: &str) -> PairedPeer {
    PairedPeer {
        instance_id: id.into(),
        instance_name: "trusted-peer".into(),
        addresses: vec![PeerAddress::from_ip("100.64.1.9")],
        api_port: 7685,
        public_key: "trusted-key".into(),
        paired_at: "now".into(),
        last_seen: None,
        online: true,
        is_primary: false,
        access_level: AccessLevel::Full,
        version: None,
        web_ui: None,
        relay_protocol_version: 0,
        consecutive_misses: 0,
        last_active_ms: None,
    }
}

fn manual_pairing_target() -> MeshManager {
    let mut mesh = MeshManager::new("test-host".into(), 7685);
    mesh.disk_persist = false;
    mesh.discovered_peers.insert(
        PLACEHOLDER.into(),
        discovered_peer(PLACEHOLDER, DiscoveryMethod::Manual),
    );
    mesh.pairing_targets.insert(
        "request-1".into(),
        ("127.0.0.1".into(), 7786, PLACEHOLDER.into()),
    );
    mesh
}

fn assert_manual_target_unchanged(mesh: &MeshManager) {
    assert_eq!(mesh.pairing_targets["request-1"].2, PLACEHOLDER);
    assert_eq!(
        mesh.discovered_peers
            .get(PLACEHOLDER)
            .expect("manual placeholder must remain")
            .instance_id,
        PLACEHOLDER
    );
}

#[test]
fn pair_request_receipt_promotes_only_a_manual_placeholder() {
    let mut mesh = manual_pairing_target();

    mesh.confirm_pairing_target_identity("request-1", REMOTE_ID)
        .unwrap();

    assert!(!mesh.discovered_peers.contains_key(PLACEHOLDER));
    assert_eq!(
        mesh.discovered_peers.get(REMOTE_ID).unwrap().instance_id,
        REMOTE_ID
    );
    assert_eq!(mesh.pairing_targets["request-1"].2, REMOTE_ID);
}

#[test]
fn pair_request_receipt_rejects_own_identity_without_mutation() {
    let mut mesh = manual_pairing_target();
    let own_id = mesh.ident.identity.instance_id.clone();

    let error = mesh
        .confirm_pairing_target_identity("request-1", &own_id)
        .unwrap_err();

    assert!(error.contains("this CodeTerm instance"));
    assert_manual_target_unchanged(&mesh);
}

#[test]
fn pair_request_receipt_rejects_paired_identity_without_mutation() {
    let mut mesh = manual_pairing_target();
    mesh.paired_peers
        .insert(REMOTE_ID.into(), paired_peer(REMOTE_ID));

    let error = mesh
        .confirm_pairing_target_identity("request-1", REMOTE_ID)
        .unwrap_err();

    assert!(error.contains("already paired"));
    assert_eq!(mesh.paired_peers[REMOTE_ID].public_key, "trusted-key");
    assert_manual_target_unchanged(&mesh);
}

#[test]
fn pair_request_receipt_rejects_blocked_and_revoked_identities_without_mutation() {
    for revoked in [false, true] {
        let mut mesh = manual_pairing_target();
        if revoked {
            mesh.revocation_list.revoked.push(REMOTE_ID.into());
        } else {
            mesh.revocation_list.blocked.push(REMOTE_ID.into());
        }

        let error = mesh
            .confirm_pairing_target_identity("request-1", REMOTE_ID)
            .unwrap_err();

        assert!(error.contains(if revoked { "revoked" } else { "blocked" }));
        assert_manual_target_unchanged(&mesh);
    }
}

#[test]
fn pair_request_receipt_rejects_discovered_identity_collision_without_mutation() {
    let mut mesh = manual_pairing_target();
    mesh.discovered_peers.insert(
        REMOTE_ID.into(),
        discovered_peer(REMOTE_ID, DiscoveryMethod::Udp),
    );

    let error = mesh
        .confirm_pairing_target_identity("request-1", REMOTE_ID)
        .unwrap_err();

    assert!(error.contains("conflicts with an existing discovery"));
    assert_eq!(
        mesh.discovered_peers[REMOTE_ID].discovery_method,
        DiscoveryMethod::Udp
    );
    assert_manual_target_unchanged(&mesh);
}

#[test]
fn pair_request_receipt_rejects_invalid_identity_without_mutation() {
    let mut mesh = manual_pairing_target();

    let error = mesh
        .confirm_pairing_target_identity("request-1", "not-a-uuid")
        .unwrap_err();

    assert!(error.contains("invalid instance identity"));
    assert_manual_target_unchanged(&mesh);
}

#[test]
fn pair_accept_validation_rejects_a_late_trusted_peer_collision() {
    let mut mesh = manual_pairing_target();
    mesh.confirm_pairing_target_identity("request-1", REMOTE_ID)
        .unwrap();
    mesh.paired_peers
        .insert(REMOTE_ID.into(), paired_peer(REMOTE_ID));

    let error = mesh
        .validate_pairing_accept_identity("request-1", REMOTE_ID)
        .unwrap_err();

    assert!(error.contains("already paired"));
    assert_eq!(mesh.paired_peers[REMOTE_ID].public_key, "trusted-key");
    assert!(!mesh.discovered_peers.contains_key(PLACEHOLDER));
}

#[test]
fn pair_request_receipt_rejects_a_discovered_identity_change() {
    let mut mesh = MeshManager::new("test-host".into(), 7685);
    mesh.disk_persist = false;
    let expected_id = "798d9973-91d2-45ae-aa6e-359b1a1cd365";
    mesh.discovered_peers.insert(
        expected_id.into(),
        discovered_peer(expected_id, DiscoveryMethod::Udp),
    );
    mesh.pairing_targets.insert(
        "request-1".into(),
        ("127.0.0.1".into(), 7786, expected_id.into()),
    );

    let error = mesh
        .confirm_pairing_target_identity("request-1", REMOTE_ID)
        .unwrap_err();

    assert!(error.contains("identity mismatch"));
    assert_eq!(mesh.pairing_targets["request-1"].2, expected_id);
    assert!(mesh.discovered_peers.contains_key(expected_id));
}
