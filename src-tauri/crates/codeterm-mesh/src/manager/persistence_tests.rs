use super::*;

/// codeterm#368 gate: the "persisted addresses don't rehydrate" hypothesis,
/// refuted — `load_peers`'s pure transform carries them straight through for
/// any well-formed IP, and the resulting manager routes with no set-addr call.
#[test]
fn paired_peer_routes_without_set_addr() {
    let payload = serde_json::json!({
        "peers": {
            "peer-a": {
                "instance_id": "peer-a",
                "instance_name": "rollacode-bot",
                "addresses": [
                    { "ip": "100.64.10.3", "is_tailscale": true },
                    { "ip": "192.168.1.251", "is_tailscale": false }
                ],
                "api_port": 7685,
                "public_key": "test-public-key",
                "paired_at": "2026-01-01T00:00:00Z",
                "last_seen": "2026-01-01T00:00:00Z",
                "relay_protocol_version": types::RELAY_PROTOCOL_VERSION
            }
        }
    })
    .to_string();

    let (mut peers, scrubbed) =
        peers::rehydrate_peers_payload(&payload).expect("valid persisted peers.json parses");
    assert_eq!(scrubbed, 0, "both addresses are well-formed IPs, nothing to scrub");
    let mut peer = peers.remove("peer-a").expect("peer-a rehydrated from the payload");
    // Liveness is re-established independently of the disk record (load_peers always
    // resets it to false); the test fixture's "online: true" is this runtime signal.
    peer.online = true;

    let mut m = MeshManager::new("test-host".into(), 7685);
    m.disk_persist = false;
    m.paired_peers.insert("peer-a".into(), peer);

    // 1. The persisted addresses actually made it into the live map.
    assert_eq!(
        m.paired_peers["peer-a"].addresses.len(),
        2,
        "persisted addresses rehydrated into paired_peers"
    );

    // 2. client_for succeeds on that record alone.
    assert!(
        m.client_for("peer-a").is_some(),
        "a paired peer with valid persisted addresses is routable without set-addr"
    );

    // 3. What the peer-list view exposes (what cli `mesh peers` renders from,
    // json and human-readable alike — both read off peers_response()) carries
    // the real address, not a placeholder.
    let response = m.peers_response();
    let rendered = response
        .paired
        .iter()
        .find(|p| p.instance_id == "peer-a")
        .expect("peer-a listed in peers_response");
    assert!(
        rendered.addresses.iter().any(|a| a.ip == "100.64.10.3"),
        "the real address renders, not a placeholder"
    );
}

/// A record that never had a routable address (never announced, never set-addr'd)
/// stays a genuinely different situation from "no paired record at all" — see
/// manager::client_for and mesh_routes::relay's distinct refusal reasons.
#[test]
fn paired_peer_with_no_address_history_is_unroutable_not_missing() {
    let mut m = MeshManager::new("test-host".into(), 7685);
    m.disk_persist = false;
    let peer = types::PairedPeer {
        instance_id: "peer-b".into(),
        instance_name: "no-address-peer".into(),
        addresses: Vec::new(),
        api_port: 7685,
        public_key: "test-public-key".into(),
        paired_at: "2026-01-01T00:00:00Z".into(),
        last_seen: None,
        online: false,
        is_primary: false,
        access_level: types::AccessLevel::Full,
        version: None,
        web_ui: None,
        relay_protocol_version: types::RELAY_PROTOCOL_VERSION,
        consecutive_misses: 0,
        last_active_ms: None,
    };
    m.paired_peers.insert("peer-b".into(), peer);

    assert!(m.paired_peers.contains_key("peer-b"), "the paired record exists");
    assert!(
        m.client_for("peer-b").is_none(),
        "but no address means no route can be built"
    );
}
