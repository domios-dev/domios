use super::*;
use crate::types::{PairedPeer, PeerAddress, PeerAnnounce, WebUiAnnouncement, WebUiEndpoint};

#[test]
fn signed_machine_identity_announce_updates_the_publisher_and_ignores_stale_or_tampered_values() {
    use crate::machine_identity::{MachineIdentity, MachineKind};
    let (private_key, public_key) = crown::generate_peer_keypair();
    let source = Arc::new(parking_lot::Mutex::new(MeshManager::in_memory(
        types::MeshIdentity {
            instance_id: "peer".into(),
            private_key: Some(private_key),
            public_key: Some(public_key.clone()),
        },
        "TestPeer".into(),
        7685,
    )));
    let mut target = make_mesh_with_peer("peer", vec![]);
    target.paired_peers.get_mut("peer").unwrap().public_key = public_key;
    let initial = source.lock().build_announce();
    let choice = MachineIdentity {
        kind: MachineKind::Laptop,
    };
    assert!(set_machine_identity(&source, choice, None).unwrap());
    let updated = source.lock().build_announce();
    assert!(
        target
            .apply_announce("peer", &updated)
            .unwrap()
            .identity_changed
    );
    assert_eq!(target.peer_machine_identity("peer"), Some(choice));
    assert!(
        !target
            .apply_announce("peer", &initial)
            .unwrap()
            .identity_changed
    );

    assert!(set_machine_identity(
        &source,
        MachineIdentity {
            kind: MachineKind::Server,
        },
        None
    )
    .unwrap());
    let mut tampered = serde_json::to_value(source.lock().build_announce()).unwrap();
    tampered["machine_identity"]["revision"]["identity"]["kind"] = serde_json::json!("vm");
    assert!(
        !target
            .apply_announce("peer", &serde_json::from_value(tampered).unwrap())
            .unwrap()
            .identity_changed
    );
    assert_eq!(target.peer_machine_identity("peer"), Some(choice));
}

#[test]
fn signed_machine_identity_interoperates_with_1_23_2_colored_announce_and_sync() {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use ed25519_dalek::{Signer, SigningKey};

    use crate::machine_identity::{MachineIdentity, MachineKind};

    #[derive(serde::Serialize)]
    struct LegacyIdentity<'a> {
        kind: &'a str,
        color: &'a str,
    }
    #[derive(serde::Serialize)]
    struct LegacyRevision<'a> {
        identity: LegacyIdentity<'a>,
        number: u64,
    }

    let (private_key, public_key) = crown::generate_peer_keypair();
    let seed: [u8; 32] = STANDARD.decode(&private_key).unwrap().try_into().unwrap();
    let signing_key = SigningKey::from_bytes(&seed);
    let sign_1_23_2 = |instance_id: &str, kind: &str, color: &str, number: u64| {
        let revision = LegacyRevision {
            identity: LegacyIdentity { kind, color },
            number,
        };
        let mut message = b"domios-mesh-machine-identity-v1\0".to_vec();
        message.extend(serde_json::to_vec(&(instance_id, &revision)).unwrap());
        let signature = signing_key.sign(&message);
        serde_json::json!({
            "revision": revision,
            "signature": STANDARD.encode(signature.to_bytes()),
        })
    };

    // Admit the peer with its real key: the identity sync is gated on the admitted key.
    let mut target = make_mesh_with_keyed_peer("peer", vec![], &public_key);

    let announce_json = serde_json::json!({
        "machine_identity": sign_1_23_2("peer", "laptop", "teal", 2),
        "instance_id": "peer",
        "instance_name": "LegacyPeer",
        "addresses": [],
        "api_port": 7685,
        "online": true,
        "relay_protocol_version": types::RELAY_PROTOCOL_VERSION,
    });
    let announce: PeerAnnounce = serde_json::from_value(announce_json).unwrap();
    assert!(
        target
            .apply_announce("peer", &announce)
            .unwrap()
            .identity_changed
    );
    assert_eq!(
        target.peer_machine_identity("peer"),
        Some(MachineIdentity {
            kind: MachineKind::Laptop,
        })
    );

    let sync_json = serde_json::json!({
        "peers": [{
            "machine_identity": sign_1_23_2("peer", "server", "pink", 3),
            "instance_id": "peer",
            "instance_name": "LegacyPeer",
            "addresses": [],
            "api_port": 7685,
            "dev": false,
        }],
        "generation": 1,
    });
    let sync: types::MeshSyncPayload = serde_json::from_value(sync_json).unwrap();
    assert!(target.apply_machine_identity_sync("peer", &sync.peers));
    assert_eq!(
        target.peer_machine_identity("peer"),
        Some(MachineIdentity {
            kind: MachineKind::Server,
        })
    );

    let mut tampered = serde_json::json!({
        "machine_identity": sign_1_23_2("peer", "vm", "violet", 4),
        "instance_id": "peer",
        "instance_name": "LegacyPeer",
        "addresses": [],
        "api_port": 7685,
        "online": true,
        "relay_protocol_version": types::RELAY_PROTOCOL_VERSION,
    });
    tampered["machine_identity"]["revision"]["identity"]["color"] = serde_json::json!("blue");
    let tampered_announce: PeerAnnounce = serde_json::from_value(tampered).unwrap();
    assert!(
        !target
            .apply_announce("peer", &tampered_announce)
            .unwrap()
            .identity_changed
    );
    assert_eq!(
        target.peer_machine_identity("peer"),
        Some(MachineIdentity {
            kind: MachineKind::Server,
        })
    );

    let source = Arc::new(parking_lot::Mutex::new(MeshManager::in_memory(
        types::MeshIdentity {
            instance_id: "peer".into(),
            private_key: Some(private_key),
            public_key: Some(public_key),
        },
        "NewPeer".into(),
        7685,
    )));
    let outbound = serde_json::to_value(source.lock().build_announce()).unwrap();
    assert_eq!(
        outbound["machine_identity"]["revision"]["identity"]["color"],
        serde_json::json!("graphite")
    );
}

#[test]
fn mesh_status_exposes_local_transport_and_web_ui_while_signed_out_without_peers() {
    use crate::network_transport::{TransportState, TransportStatus};
    let mut manager = MeshManager::in_memory(
        types::MeshIdentity {
            instance_id: "local".into(),
            private_key: None,
            public_key: None,
        },
        "Local".into(),
        7685,
    );
    assert!(manager.account.signed_in_account().is_none());
    let empty = serde_json::to_value(manager.status()).unwrap();
    assert_eq!(empty["paired_count"], 0);
    assert_eq!(empty.get("transport"), Some(&serde_json::Value::Null));
    assert_eq!(empty.get("web_ui"), Some(&serde_json::Value::Null));
    manager.set_network_transport(TransportStatus {
        display_name: "Tailscale".into(),
        state: TransportState::Connected,
        identity: None,
        network_name: None,
        user: Some("private-login".into()),
        addresses: vec![concat!("100.", "64.0.7").parse().unwrap()],
        peer_listener: true,
        encrypted: true,
        setup_actions: vec![],
    });
    manager.local_web_ui = Some(WebUiAnnouncement {
        protocol_version: types::RELAY_PROTOCOL_VERSION,
        endpoints: vec![WebUiEndpoint::TailscaleHttps {
            host: concat!("node.fixture.ts", ".net").into(),
            port: 443,
        }],
    });
    let value = serde_json::to_value(manager.status()).unwrap();
    assert_eq!(value["paired_count"], 0);
    assert_eq!(value["transport"]["state"], "connected");
    assert_eq!(
        value["transport"]["addresses"],
        serde_json::json!([concat!("100.", "64.0.7")])
    );
    assert_eq!(
        value["web_ui"]["endpoints"],
        serde_json::json!([
            {"transport": "tailscale_https", "host": concat!("node.fixture.ts", ".net"), "port": 443}
        ])
    );
    assert!(value["transport"].get("user").is_none());
    assert!(manager.account.signed_in_account().is_none());
}

fn make_mesh_with_peer(peer_id: &str, addrs: Vec<&str>) -> MeshManager {
    make_mesh_with_keyed_peer(peer_id, addrs, "")
}

fn make_mesh_with_keyed_peer(peer_id: &str, addrs: Vec<&str>, public_key: &str) -> MeshManager {
    let mut m = MeshManager::new("test-host".into(), 7685);
    m.disk_persist = false;
    let peer = PairedPeer {
        instance_id: peer_id.into(),
        instance_name: "TestPeer".into(),
        addresses: addrs.into_iter().map(PeerAddress::from_ip).collect(),
        api_port: 7685,
        public_key: public_key.into(),
        paired_at: chrono::Utc::now().to_rfc3339(),
        last_seen: None,
        online: true,
        is_primary: false,
        access_level: types::AccessLevel::Full,
        domios_version: None,
        domios_build: None,
        web_ui: None,
        relay_protocol_version: types::RELAY_PROTOCOL_VERSION,
        consecutive_misses: 0,
        last_active_ms: None,
    };
    m.insert_account_peer_for_test(peer);
    m
}

#[test]
fn peer_inventory_uses_authenticated_lease_without_mutating_cached_registry() {
    let mut manager = make_mesh_with_peer("peer-a", vec![]);
    manager.set_clock(Arc::new(|| 100));
    manager.account_installations[0].membership_expires_at = Some(99);
    let expiry = manager
        .link_record("peer-a")
        .unwrap()
        .remote_membership
        .as_ref()
        .unwrap()
        .verified
        .expires_at;
    assert_eq!(
        manager.peers_response().installations[0].membership_expires_at,
        Some(expiry)
    );
    assert_eq!(
        manager.account_installations[0].membership_expires_at,
        Some(99)
    );

    let mut state = manager.account_state().clone();
    state.unavailable_installations.insert("peer-a".into());
    manager.apply_account_state(state.clone());
    assert_eq!(
        manager.peers_response().installations[0].membership_expires_at,
        Some(99)
    );
    state.unavailable_installations.clear();
    state.revoked_installations.insert("peer-a".into());
    manager.apply_account_state(state.clone());
    assert_eq!(manager.peer_membership_expiry("peer-a"), None);
    state.revoked_installations.clear();
    manager.apply_account_state(state);
    manager.set_clock(Arc::new(move || expiry));
    assert_eq!(manager.peer_membership_expiry("peer-a"), None);
}

#[test]
fn peers_response_omits_inactive_installations_and_removes_installation_pairing() {
    let mut manager = make_mesh_with_peer("peer-a", vec![]);
    manager
        .account_installations
        .push(types::AccountInstallation {
            installation_id: "stale-removed".into(),
            name: "Stale".into(),
            active: false,
            signed_out: false,
            membership_expires_at: None,
            endpoint_expires_at: None,
            instance_id: None,
            online: false,
        });
    let response = manager.peers_response();
    assert_eq!(response.installations.len(), 1);
    assert_eq!(response.installations[0].installation_id, "peer-a");

    manager.remove_account_installation("peer-a");
    assert!(manager.paired_peers.is_empty());
}

#[test]
fn apply_announce_requires_a_signed_endpoint_record_to_replace_addresses() {
    let mut m = make_mesh_with_peer("peer-A", vec!["192.168.1.5", "100.64.0.5"]);
    let ann = PeerAnnounce {
        inbound_only: false,
        machine_identity: None,
        domios_version: None,
        domios_build: None,
        instance_id: "peer-A".into(),
        instance_name: "TestPeer".into(),
        addresses: vec![PeerAddress::from_ip("100.99.99.99")],
        api_port: 7777,
        online: true,
        web_ui: None,
        relay_protocol_version: types::RELAY_PROTOCOL_VERSION,
    };
    let change = m.apply_announce("peer-A", &ann).expect("ok");
    assert!(!change.addresses_changed);
    assert_eq!(
        change.previous_addresses.len(),
        2,
        "old list returned for logging"
    );
    let after = &m.paired_peers["peer-A"];
    assert_eq!(after.addresses.len(), 2);
    assert_eq!(after.addresses[0].ip, "192.168.1.5");
    assert_ne!(after.api_port, 7777);
}

#[test]
fn apply_announce_stores_and_clears_web_ui() {
    let mut m = make_mesh_with_peer("peer-A", vec!["100.64.0.5"]);
    let with_https = PeerAnnounce {
        inbound_only: false,
        machine_identity: None,
        domios_version: None,
        domios_build: None,
        instance_id: "peer-A".into(),
        instance_name: "TestPeer".into(),
        addresses: vec![PeerAddress::from_ip("100.64.0.5")],
        api_port: 7685,
        online: true,
        web_ui: Some(WebUiAnnouncement {
            protocol_version: types::RELAY_PROTOCOL_VERSION,
            endpoints: vec![WebUiEndpoint::TailscaleHttps {
                host: "peer-a.tail.ts.net".into(),
                port: 443,
            }],
        }),
        relay_protocol_version: types::RELAY_PROTOCOL_VERSION,
    };
    let added = m.apply_announce("peer-A", &with_https).expect("ok");
    assert!(added.web_ui_changed);
    assert_eq!(m.paired_peers["peer-A"].web_ui, with_https.web_ui);

    let without = PeerAnnounce {
        inbound_only: false,
        machine_identity: None,
        web_ui: None,
        relay_protocol_version: types::RELAY_PROTOCOL_VERSION,
        ..with_https
    };
    let cleared = m.apply_announce("peer-A", &without).expect("ok");
    assert!(cleared.web_ui_changed);
    assert_eq!(m.paired_peers["peer-A"].web_ui, None);
}

#[test]
fn apply_announce_rejects_spoofed_sender() {
    let mut m = make_mesh_with_peer("peer-A", vec!["192.168.1.5"]);
    let ann = PeerAnnounce {
        inbound_only: false,
        machine_identity: None,
        domios_version: None,
        domios_build: None,
        instance_id: "peer-A".into(),
        instance_name: "TestPeer".into(),
        addresses: vec![PeerAddress::from_ip("100.99.99.99")],
        api_port: 7685,
        online: true,
        web_ui: None,
        relay_protocol_version: types::RELAY_PROTOCOL_VERSION,
    };
    let err = m.apply_announce("peer-B", &ann).unwrap_err();
    assert!(err.contains("cannot announce"), "got: {}", err);
    assert_eq!(
        m.paired_peers["peer-A"].addresses[0].ip, "192.168.1.5",
        "untouched"
    );
}

#[test]
fn apply_announce_empty_list_is_noop() {
    let mut m = make_mesh_with_peer("peer-A", vec!["192.168.1.5"]);
    let ann = PeerAnnounce {
        inbound_only: false,
        machine_identity: None,
        domios_version: None,
        domios_build: None,
        instance_id: "peer-A".into(),
        instance_name: "TestPeer".into(),
        addresses: vec![],
        api_port: 7685,
        online: true,
        web_ui: None,
        relay_protocol_version: types::RELAY_PROTOCOL_VERSION,
    };
    let res = m.apply_announce("peer-A", &ann).expect("ok");
    assert!(!res.changed(), "empty list does not clear cache");
    assert_eq!(m.paired_peers["peer-A"].addresses.len(), 1);
}

#[test]
fn apply_announce_same_addrs_returns_no_change() {
    let mut m = make_mesh_with_peer("peer-A", vec!["100.64.0.5"]);
    let ann = PeerAnnounce {
        inbound_only: false,
        machine_identity: None,
        domios_version: None,
        domios_build: None,
        instance_id: "peer-A".into(),
        instance_name: "TestPeer".into(),
        addresses: vec![PeerAddress::from_ip("100.64.0.5")],
        api_port: 7685,
        online: true,
        web_ui: None,
        relay_protocol_version: types::RELAY_PROTOCOL_VERSION,
    };
    let res = m.apply_announce("peer-A", &ann).expect("ok");
    assert!(!res.changed(), "identical list reports no change");
}

fn announce_with_protocol(relay_protocol_version: u16) -> PeerAnnounce {
    PeerAnnounce {
        inbound_only: false,
        machine_identity: None,
        domios_version: None,
        domios_build: None,
        instance_id: "peer-A".into(),
        instance_name: "TestPeer".into(),
        addresses: vec![PeerAddress::from_ip("100.64.0.5")],
        api_port: 7685,
        online: true,
        web_ui: None,
        relay_protocol_version,
    }
}

#[test]
fn apply_announce_refreshes_a_stored_v1_record_from_a_supported_announce() {
    let mut m = make_mesh_with_peer("peer-A", vec!["100.64.0.5"]);
    m.paired_peers
        .get_mut("peer-A")
        .unwrap()
        .relay_protocol_version = 1;

    let change = m
        .apply_announce(
            "peer-A",
            &announce_with_protocol(types::RELAY_PROTOCOL_VERSION),
        )
        .expect("supported announce accepted");

    assert!(change.metadata_changed);
    assert_eq!(
        m.paired_peers["peer-A"].relay_protocol_version,
        types::RELAY_PROTOCOL_VERSION
    );
}

#[test]
fn apply_announce_refuses_an_unsupported_protocol_and_keeps_the_record() {
    let mut m = make_mesh_with_peer("peer-A", vec!["192.168.1.5"]);
    m.paired_peers
        .get_mut("peer-A")
        .unwrap()
        .relay_protocol_version = 1;

    let err = m
        .apply_announce("peer-A", &announce_with_protocol(1))
        .unwrap_err();

    assert!(err.contains("upgrade both peers"), "got: {err}");
    let peer = &m.paired_peers["peer-A"];
    assert_eq!(peer.relay_protocol_version, 1);
    assert_eq!(peer.addresses[0].ip, "192.168.1.5");
}

#[test]
fn set_peer_addr_override_replaces_with_single_entry() {
    let mut m = make_mesh_with_peer("peer-A", vec!["192.168.1.5", "100.64.0.5"]);
    let prev = m
        .set_peer_addr_override("peer-A", "10.0.0.99", Some(8888))
        .expect("ok");
    assert_eq!(prev.len(), 2);
    let after = &m.paired_peers["peer-A"];
    assert_eq!(after.addresses.len(), 1);
    assert_eq!(after.addresses[0].ip, "10.0.0.99");
    assert_eq!(after.api_port, 8888);
}

#[test]
fn set_peer_addr_override_unknown_peer_errors() {
    let mut m = MeshManager::new("test-host".into(), 7685);
    m.disk_persist = false;
    let err = m
        .set_peer_addr_override("ghost", "10.0.0.1", None)
        .unwrap_err();
    assert!(err.contains("not paired"), "got: {}", err);
}

#[test]
fn build_announce_includes_tailscale_and_lan() {
    let mut m = MeshManager::new("test-host".into(), 7685);
    m.ready_listeners = vec!["0.0.0.0:7685".parse().unwrap()];
    m.disk_persist = false;
    m.tailscale_ip = Some("100.65.0.30".into());
    m.local_ip = Some("192.168.1.5".into());
    let ann = m.build_announce();
    assert_eq!(ann.addresses.len(), 2);
    assert!(ann.addresses[0].is_tailscale, "tailscale first");
    assert_eq!(ann.addresses[0].ip, "100.65.0.30");
    assert_eq!(ann.addresses[1].ip, "192.168.1.5");
    assert!(!ann.addresses[1].is_tailscale);
    assert_eq!(ann.api_port, 7685);
}

#[test]
fn build_announce_skips_public_local_ip() {
    let mut m = MeshManager::new("test-host".into(), 7685);
    m.disk_persist = false;
    m.tailscale_ip = None;
    m.local_ip = Some("8.8.8.8".into());
    let ann = m.build_announce();
    assert!(ann.addresses.is_empty(), "public IPs are not advertised");
}

#[test]
fn loopback_listener_with_verified_serve_announces_online() {
    let mut m = MeshManager::new("test-host".into(), 7685);
    m.disk_persist = false;
    m.local_http_healthy = true;
    m.ready_listeners = vec!["127.0.0.1:7685".parse().unwrap()];
    m.tailscale_ip = Some("100.65.0.30".into());
    m.local_web_ui = Some(WebUiAnnouncement {
        protocol_version: types::RELAY_PROTOCOL_VERSION,
        endpoints: vec![WebUiEndpoint::TailscaleHttp {
            ip: "100.65.0.30".parse().unwrap(),
            port: 7685,
        }],
    });

    let announcement = m.build_announce();

    assert!(announcement.online);
    assert_eq!(announcement.addresses.len(), 1);
    assert_eq!(announcement.addresses[0].ip, "100.65.0.30");
    assert!(announcement.addresses[0].is_tailscale);
    assert_eq!(announcement.web_ui, m.local_web_ui);
}

#[test]
fn loopback_listener_without_verified_serve_stays_offline() {
    let mut m = MeshManager::new("test-host".into(), 7685);
    m.disk_persist = false;
    m.local_http_healthy = true;
    m.ready_listeners = vec!["127.0.0.1:7685".parse().unwrap()];
    m.tailscale_ip = Some("100.65.0.30".into());

    let announcement = m.build_announce();

    assert!(!announcement.online);
    assert!(announcement.addresses.is_empty());
    assert!(announcement.web_ui.is_none());
}

#[test]
fn mesh_reachability_remote_loss_withdraws_paired_cached_addresses() {
    let mut sender = MeshManager::new("sender".into(), 7690);
    sender.disk_persist = false;
    sender.local_http_healthy = true;
    sender.ready_listeners = vec!["127.0.0.1:7690".parse().unwrap()];
    let id = sender.ident.identity.instance_id.clone();
    let mut receiver = make_mesh_with_peer(&id, vec!["100.64.0.1"]);
    let announcement = sender.build_announce();
    assert!(!announcement.online);
    assert!(announcement.addresses.is_empty());
    receiver.apply_announce(&id, &announcement).unwrap();
    assert!(receiver.paired_peers[&id].addresses.is_empty());
    assert!(!receiver.paired_peers[&id].online);
}

#[test]
fn client_for_picks_tailscale_via_tier_ranking() {
    let mut m = MeshManager::new("test-host".into(), 7685);
    m.disk_persist = false;
    let peer = PairedPeer {
        instance_id: "peer-A".into(),
        instance_name: "TestPeer".into(),
        addresses: vec![
            PeerAddress {
                ip: "8.8.8.8".into(),
                is_tailscale: false,
            },
            PeerAddress {
                ip: "192.168.1.2".into(),
                is_tailscale: false,
            },
            PeerAddress {
                ip: "100.65.0.30".into(),
                is_tailscale: true,
            },
        ],
        api_port: 7685,
        public_key: String::new(),
        paired_at: chrono::Utc::now().to_rfc3339(),
        last_seen: None,
        online: true,
        is_primary: false,
        access_level: types::AccessLevel::Full,
        domios_version: None,
        domios_build: None,
        web_ui: None,
        relay_protocol_version: 0,
        consecutive_misses: 0,
        last_active_ms: None,
    };
    m.insert_account_peer_for_test(peer);

    let picked = m.paired_peers["peer-A"]
        .addresses
        .iter()
        .min_by_key(|a| types::address_tier(a))
        .unwrap();
    assert_eq!(picked.ip, "100.65.0.30");
}

#[test]
fn mesh_rename_announce_stays_queued_until_the_peer_acknowledges() {
    let mut m = make_mesh_with_peer("peer-1", vec!["100.64.0.9"]);
    assert!(!m.has_unacked_announce("peer-1"));
    m.record_announce_outcome("peer-1", false);
    assert!(m.has_unacked_announce("peer-1"));
    m.record_announce_outcome("peer-1", false);
    assert!(
        m.has_unacked_announce("peer-1"),
        "a repeat failure keeps one entry"
    );
    m.record_announce_outcome("peer-1", true);
    assert!(!m.has_unacked_announce("peer-1"));
}

#[test]
fn announce_carries_and_retains_peer_product_version() {
    let mut manager = make_mesh_with_peer("peer-A", vec![]);
    manager.build_hash = "66576d9".into();
    let local = manager.build_announce();
    assert_eq!(
        local.domios_version.as_deref(),
        Some(crate::version::local_version())
    );
    assert_eq!(local.domios_build.as_deref(), Some("66576d9"));
    let mut announce = announce_with_protocol(types::RELAY_PROTOCOL_VERSION);
    announce.instance_id = "peer-A".into();
    announce.domios_version = Some("1.20.0".into());
    announce.domios_build = Some("1234567".into());
    assert!(
        manager
            .apply_announce("peer-A", &announce)
            .unwrap()
            .metadata_changed
    );
    let encoded = serde_json::to_value(&manager.paired_peers["peer-A"]).unwrap();
    assert_eq!(encoded["domios_version"], "1.20.0");
    assert_eq!(encoded["domios_build"], "1234567");
    announce.domios_version = None;
    announce.domios_build = None;
    manager.apply_announce("peer-A", &announce).unwrap();
    assert_eq!(
        manager.paired_peers["peer-A"].domios_version.as_deref(),
        Some("1.20.0")
    );
    assert_eq!(
        manager.paired_peers["peer-A"].domios_build.as_deref(),
        Some("1234567")
    );
    announce.domios_version = Some("1.20.1".into());
    announce.domios_build = Some("abcdef0".into());
    assert!(
        manager
            .apply_announce("peer-A", &announce)
            .unwrap()
            .metadata_changed
    );
    assert_eq!(
        manager.paired_peers["peer-A"].domios_version.as_deref(),
        Some("1.20.1")
    );
    assert_eq!(
        manager.paired_peers["peer-A"].domios_build.as_deref(),
        Some("abcdef0")
    );
}
