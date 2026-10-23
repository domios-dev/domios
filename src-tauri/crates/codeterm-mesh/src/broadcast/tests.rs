use super::peer_request_error;

#[test]
fn peer_request_error_keeps_identity_and_drops_urls() {
    let error = peer_request_error(
        "peer-id",
        "failed to connect to http://192.0.2.10:7685/api/mesh/announce",
    );

    assert_eq!(error, "peer-id: request failed");
    assert!(!error.contains("192.0.2.10"));
    assert!(!error.contains("http://"));
}
