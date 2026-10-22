use super::{sync_input_for_access, sync_output_for_access};
use codeterm_service_core::mesh::types::{GossipPeerInfo, MeshSyncPayload, PeerAddress};

fn peer(instance_id: &str) -> GossipPeerInfo {
    GossipPeerInfo {
        instance_id: instance_id.into(),
        instance_name: instance_id.into(),
        addresses: vec![PeerAddress::from_ip("100.64.0.1")],
        api_port: 7685,
        dev: false,
        last_active_ms: Some(17),
    }
}

#[test]
fn lower_access_sync_keeps_only_the_sender_identity_and_liveness() {
    let peers = [peer("sender"), peer("introduced")];
    let allowed = sync_input_for_access(false, Some("sender"), &peers);
    assert_eq!(
        allowed
            .iter()
            .map(|entry| entry.instance_id.as_str())
            .collect::<Vec<_>>(),
        ["sender"]
    );

    let response = sync_output_for_access(
        false,
        "receiver",
        MeshSyncPayload {
            peers: vec![peer("receiver"), peer("other")],
            generation: 9,
        },
    );
    assert_eq!(response.peers.len(), 1);
    assert_eq!(response.peers[0].instance_id, "receiver");
    assert_eq!(response.peers[0].last_active_ms, Some(17));
}

#[test]
fn mesh_gossip_never_relearns_its_own_identity_or_known_peers() {
    let mut mesh = codeterm_service_core::mesh::MeshManager::in_memory(
        codeterm_service_core::mesh::types::MeshIdentity {
            instance_id: "self".into(),
            private_key: None,
            public_key: None,
        },
        "BLACKSTAR".into(),
        7685,
    );
    let peers = [peer("self"), peer("sender"), peer("introduced")];

    let first = super::known_peer_ids(&mesh);
    let learned: Vec<_> = super::newly_learned_peers(&first, &peers)
        .map(|peer| peer.instance_id.as_str())
        .collect();
    assert_eq!(learned, ["sender", "introduced"]);

    mesh.apply_gossip_snapshot("sender", 1, &peers, 10);
    let second = super::known_peer_ids(&mesh);
    assert_eq!(
        super::newly_learned_peers(&second, &peers).count(),
        0,
        "a repeated exchange learns nothing"
    );
}

#[test]
fn full_access_sync_keeps_peer_introductions() {
    let peers = [peer("sender"), peer("introduced")];
    let allowed = sync_input_for_access(true, Some("sender"), &peers);
    assert_eq!(allowed.len(), 2);
}
