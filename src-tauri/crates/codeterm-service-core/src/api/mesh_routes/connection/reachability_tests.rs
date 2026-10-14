use super::*;

fn manager() -> crate::mesh::MeshManager {
    let mut manager = crate::mesh::MeshManager::new("fixture".into(), 7690);
    manager.disk_persist = false;
    manager.ident.identity.public_key = Some("test-key".into());
    manager.discovered_peers.insert(
        "peer".into(),
        crate::mesh::types::DiscoveredPeer {
            instance_id: "peer".into(),
            instance_name: "peer".into(),
            addresses: vec![crate::mesh::types::PeerAddress::from_ip("100.64.0.2")],
            api_port: 7691,
            discovery_method: crate::mesh::types::DiscoveryMethod::Manual,
            last_seen: 0,
            dev: true,
            mesh_id: None,
            vouched_by: Vec::new(),
        },
    );
    manager
}

#[test]
fn mesh_reachability_connect_refuses_unreachable_callback_without_reservation() {
    let mut manager = manager();
    manager.ready_listeners = vec!["127.0.0.1:7690".parse().unwrap()];
    assert!(matches!(prepare_connect(&mut manager, "peer"),
        Err(ConnectPeerError::BadRequest(message)) if message == "mesh_listener_not_remotely_ready"));
    assert!(manager.pending_requests.is_empty());
    assert!(manager.pairing_targets.is_empty());
    assert!(manager.pairing_deliveries.is_empty());
}

#[test]
fn mesh_reachability_connect_carries_only_live_remote_callback() {
    let mut manager = manager();
    manager.local_ip = Some("192.168.1.2".into());
    manager.ready_listeners = vec![
        "127.0.0.1:7690".parse().unwrap(),
        "100.64.0.1:7690".parse().unwrap(),
    ];
    let ConnectPreparation::Send { request, .. } = prepare_connect(&mut manager, "peer").unwrap()
    else {
        panic!("fresh pairing must prepare a request");
    };
    assert_eq!(request.from_address, "100.64.0.1");
    assert_eq!(request.from_addresses.len(), 1);
    assert_eq!(request.from_addresses[0].ip, "100.64.0.1");
    assert_eq!(request.from_port, 7690);
}
