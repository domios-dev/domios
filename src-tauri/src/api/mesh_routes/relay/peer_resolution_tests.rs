use super::*;
use crate::mesh::types::RelayResponse;

const LIVE_ID: &str = "dc737738-aaaa-bbbb-cccc-000000000001";

fn peer(id: &str, name: &str, online: bool) -> crate::mesh::types::PairedPeer {
    crate::mesh::types::PairedPeer {
        instance_id: id.into(),
        instance_name: name.into(),
        addresses: vec![crate::mesh::types::PeerAddress::from_ip("100.65.0.30")],
        api_port: 7685,
        public_key: String::new(),
        paired_at: "now".into(),
        last_seen: None,
        online,
        is_primary: false,
        access_level: crate::mesh::types::AccessLevel::Full,
        version: None,
        web_ui: None,
        relay_protocol_version: crate::mesh::types::RELAY_PROTOCOL_VERSION,
        consecutive_misses: 0,
        last_active_ms: None,
    }
}

fn mesh_with(peers: Vec<crate::mesh::types::PairedPeer>) -> crate::mesh::MeshManager {
    let mut mesh = crate::mesh::MeshManager::new("test-host".into(), 7685);
    mesh.disk_persist = false;
    mesh.paired_peers.clear();
    for peer in peers {
        mesh.paired_peers.insert(peer.instance_id.clone(), peer);
    }
    mesh
}

#[test]
fn mesh_peer_resolution_table() {
    let mesh = mesh_with(vec![
        peer(LIVE_ID, "BLACKSTAR-peer [dev-meshpeer]", true),
        peer("3b6e44a3-0000-0000-0000-000000000002", "OTHER", true),
    ]);
    for target in [
        LIVE_ID,
        "dc737738",
        "BLACKSTAR-peer",
        "blackstar-peer [dev-meshpeer]",
    ] {
        let resolved = match resolve_relay_peer(&mesh, target) {
            Ok(resolved) => resolved,
            Err(error) => panic!("{target} must resolve, got {error:?}"),
        };
        assert_eq!(resolved.1, LIVE_ID, "{target}");
    }
}

#[test]
fn mesh_peer_resolution_refuses_ambiguous_names_with_candidates() {
    let mesh = mesh_with(vec![
        peer("id-aaaa-1", "ROG", true),
        peer("id-bbbb-2", "ROG", true),
    ]);
    let (status, body) = match resolve_relay_peer(&mesh, "ROG") {
        Ok(_) => panic!("two online peers under one name must not resolve"),
        Err(error) => error,
    };
    assert_eq!(status, StatusCode::CONFLICT);
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("structured refusal");
    assert_eq!(parsed["reason"], PEER_NAME_AMBIGUOUS_REASON);
    let ids: Vec<&str> = parsed["candidates"]
        .as_array()
        .expect("candidates")
        .iter()
        .map(|c| c["instance_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["id-aaaa-1", "id-bbbb-2"]);
    assert_eq!(
        parsed["candidates"][0]["route_by_id"]["target_peer_id"],
        "id-aaaa-1"
    );
}

#[test]
fn mesh_relay_answer_names_the_answering_peer_without_changing_the_relay_shape() {
    let response = PeerRelayResponse {
        response: RelayResponse {
            ok: true,
            status: 200,
            data: Some(serde_json::json!({"tabs": []})),
            error: None,
        },
        peer: AnsweringPeer {
            instance_id: LIVE_ID.into(),
            instance_name: "BLACKSTAR-peer [dev-meshpeer]".into(),
        },
    };
    let value = serde_json::to_value(&response).unwrap();
    assert_eq!(value["ok"], true);
    assert_eq!(value["status"], 200);
    assert_eq!(value["data"]["tabs"], serde_json::json!([]));
    assert_eq!(value["peer"]["instance_id"], LIVE_ID);
    let plain: RelayResponse = serde_json::from_value(value).expect("old readers still parse it");
    assert!(plain.ok);
}
