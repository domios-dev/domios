use super::*;

#[test]
fn pairing_routes_refuse_an_old_peer_version() {
    let mut headers = HeaderMap::new();
    headers.insert(
        codeterm_service_core::mesh::version::VERSION_HEADER,
        "1.14.9".parse().unwrap(),
    );

    let (status, _) = require_current_peer_version(&headers).unwrap_err();
    assert_eq!(status, StatusCode::UPGRADE_REQUIRED);
}

#[test]
fn pairing_routes_refuse_a_missing_peer_version() {
    let (status, _) = require_current_peer_version(&HeaderMap::new()).unwrap_err();
    assert_eq!(status, StatusCode::UPGRADE_REQUIRED);
}
