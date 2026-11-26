use super::*;

#[test]
fn mesh_request_shape_is_attributed_without_exposing_the_credential() {
    let (private, _) = codeterm_service_core::mesh::crown::generate_peer_keypair();
    let token = codeterm_service_core::mesh::crown::sign_request(
        &private,
        "d99fbf3a-cfb1-4eb3-9b6b-7cfc9baff992",
    )
    .unwrap();
    assert_eq!(presented_kind(&token), "mesh-signature");
    assert_eq!(
        presented_kind("opaque-owner-or-legacy-credential"),
        "bearer"
    );
    assert_eq!(presented_kind("bad.payload"), "bearer");
    assert_eq!(presented_kind(""), "missing");
}

#[test]
fn clock_skew_verify_errors_are_distinguished_from_unknown_credentials() {
    assert_eq!(
        classify_verify_error("Request expired"),
        "clock_skew_expired"
    );
    assert_eq!(
        classify_verify_error("Request timestamp too far in the future"),
        "clock_skew_future"
    );
    assert_eq!(
        classify_verify_error("Unknown peer: peer-1"),
        "unknown_or_invalid_credential"
    );
    assert_eq!(
        classify_verify_error("Invalid signature"),
        "unknown_or_invalid_credential"
    );
}
