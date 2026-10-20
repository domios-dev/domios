use super::*;

#[test]
fn pairing_routes_refuse_a_1_14_9_peer_with_an_updater_message() {
    let mut headers = HeaderMap::new();
    headers.insert(
        codeterm_service_core::mesh::version::VERSION_HEADER,
        "1.14.9".parse().unwrap(),
    );

    let (_, message) = require_current_peer_version(&headers).unwrap_err();

    assert!(message.contains("both peers on Domios"));
    assert!(message.contains("upgrade both peers through the updater"));
    assert!(message.contains("Peer reported version 1.14.9."));
}

#[test]
fn pairing_routes_refuse_a_missing_peer_version_without_hanging() {
    let (_, message) = require_current_peer_version(&HeaderMap::new()).unwrap_err();

    assert!(message.contains("both peers on Domios"));
    assert!(message.contains("upgrade both peers through the updater"));
    assert!(!message.contains("Peer reported version"));
}

#[test]
fn current_pairing_responses_advertise_the_mesh_peer_version() {
    let headers = current_peer_version_header();
    let version = headers
        .get(codeterm_service_core::mesh::version::VERSION_HEADER)
        .unwrap()
        .to_str()
        .unwrap();

    assert!(codeterm_service_core::mesh::version::peer_version_error(Some(version)).is_none());
}
