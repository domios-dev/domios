use super::*;
use serde_json::json;

#[test]
fn macsys_localapi_status_supplies_connected_listener_addresses() {
    let value = json!({
        "BackendState": "Running",
        "Self": {
            "TailscaleIPs": ["192.0.2.1", "2001:db8::1"],
            "DNSName": "host.example.ts.net."
        }
    });
    let status = tailscale::project(true, Some(&value));
    assert_eq!(status.state, TransportState::Connected);
    assert_eq!(
        listener_addresses(&status, true, 7685),
        vec![
            "192.0.2.1:7685".parse::<SocketAddr>().unwrap(),
            "[2001:db8::1]:7685".parse::<SocketAddr>().unwrap(),
        ]
    );
}

#[test]
fn network_transport_projects_backend_states_and_keeps_identity_private() {
    for (backend, expected) in [
        ("Running", TransportState::Connected),
        ("Stopped", TransportState::NotRunning),
        ("NeedsLogin", TransportState::SignedOut),
        ("NeedsMachineAuth", TransportState::Unknown),
        ("Starting", TransportState::Starting),
        ("Unexpected", TransportState::Unknown),
    ] {
        let value = json!({"BackendState": backend,
            "CurrentTailnet": {"Name": "private@example.com", "MagicDNSSuffix": "TAIL123.ts.net."},
            "Self": {"TailscaleIPs": ["100.64.1.2", "fd7a:115c:a1e0::2"], "UserID": 42},
            "User": {"42": {"LoginName": "private@example.com"}}});
        let status = tailscale::project(true, Some(&value));
        assert_eq!(status.state, expected);
        assert_eq!(status.addresses.len(), 2);
        assert_eq!(status.user.as_deref(), Some("private@example.com"));
        let identity = serde_json::to_string(&status.identity).unwrap();
        assert!(!identity.contains("example.com"));
        assert!(!identity.contains("ts.net"));
        assert_eq!(status.identity.unwrap().network_id.len(), 64);
    }
    assert_eq!(
        tailscale::project(false, None).state,
        TransportState::NotInstalled
    );
    assert_eq!(
        tailscale::project(true, None).state,
        TransportState::NotRunning
    );
}

struct OtherTransport;
impl NetworkTransportProvider for OtherTransport {
    fn detect(&self) -> TransportStatus {
        TransportStatus {
            display_name: "Second network".into(),
            state: TransportState::Connected,
            identity: Some(NetworkIdentity {
                kind: "second".into(),
                network_id: "network".into(),
            }),
            network_name: None,
            user: None,
            addresses: vec!["10.23.1.2".parse().unwrap()],
            peer_listener: true,
            encrypted: true,
            setup_actions: Vec::new(),
        }
    }
}

#[test]
fn network_transport_core_selects_capabilities_and_provider_addresses() {
    let mut status = OtherTransport.detect();
    assert_eq!(
        listener_addresses(&status, true, 7685),
        vec!["10.23.1.2:7685".parse::<SocketAddr>().unwrap()]
    );
    assert!(listener_addresses(&status, false, 7685).is_empty());
    status.peer_listener = false;
    assert!(listener_addresses(&status, true, 7685).is_empty());
    status.peer_listener = true;
    status.state = TransportState::NotRunning;
    assert!(listener_addresses(&status, true, 7685).is_empty());
}
