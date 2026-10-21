use super::*;

#[test]
fn mesh_reachability_udp_requires_eligible_packet_source_and_nonzero_port() {
    let source = "100.64.0.2".parse().unwrap();
    let mut beacon = Beacon {
        id: "peer".into(),
        name: "peer".into(),
        port: 7690,
        dev: true,
        addresses: vec![
            PeerAddress::from_ip("100.64.0.2"),
            PeerAddress::from_ip("192.168.1.2"),
        ],
        mesh_id: None,
    };
    let admitted = udp_source_addresses(&beacon, source).unwrap();
    assert_eq!(admitted.len(), 1);
    assert_eq!(admitted[0].ip, "100.64.0.2");
    assert!(udp_source_addresses(&beacon, "100.64.0.3".parse().unwrap()).is_none());
    beacon.port = 0;
    assert!(udp_source_addresses(&beacon, source).is_none());
    beacon.port = 7690;
    for invalid in [
        "not-an-ip",
        "127.0.0.1",
        "0.0.0.0",
        "224.0.0.1",
        "169.254.1.1",
        "fe80::1",
        "::1",
    ] {
        beacon.addresses = vec![PeerAddress::from_ip(invalid)];
        assert!(udp_source_addresses(&beacon, source).is_none());
    }
    beacon.addresses = vec![PeerAddress::from_ip("100.64.0.2"); 17];
    assert!(udp_source_addresses(&beacon, source).is_none());
}

#[test]
fn mesh_reachability_broadcast_refuses_without_changing_state() {
    let mut manager = MeshManager::new("test".into(), 7690);
    manager.disk_persist = false;
    manager.local_ip = Some("192.168.1.2".into());
    manager.ready_listeners = vec!["127.0.0.1:7690".parse().unwrap()];
    let mesh = Arc::new(Mutex::new(manager));
    assert_eq!(
        start_broadcast(mesh.clone(), 180).unwrap_err(),
        "mesh_listener_not_remotely_ready"
    );
    let state = mesh.lock();
    assert!(!state.broadcast_active);
    assert_eq!(state.broadcast_expires_at, None);
    assert!(state.mdns_daemon.is_none());
}

#[test]
fn mesh_reachability_mdns_metadata_and_exact_addresses_roundtrip() {
    for dev in [true, false] {
        let properties = mdns_properties("instance-id".into(), "fixture".into(), 7690, dev);
        let service = mdns_sd::ServiceInfo::new(
            SERVICE_TYPE,
            "fixture",
            "fixture.local.",
            "100.64.0.2",
            7690,
            properties,
        )
        .unwrap();
        assert_eq!(
            service.get_property_val_str("dev"),
            Some(if dev { "true" } else { "false" })
        );
        assert_eq!(service.get_addresses().len(), 1);
        assert!(service
            .get_addresses()
            .iter()
            .any(|ip| ip.to_string() == "100.64.0.2"));
    }
}
