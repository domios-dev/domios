use super::*;

#[test]
fn mesh_request_shape_is_attributed_without_exposing_the_credential() {
    let (private, _) = codeterm_service_core::mesh::crown::generate_peer_keypair();
    let token = codeterm_service_core::mesh::crown::sign_request(
        &private,
        "d99fbf3a-cfb1-4eb3-9b6b-7cfc9baff992",
    )
    .unwrap();
    assert_eq!(token.len(), 261);
    assert_eq!(presented_kind(&token), "mesh-signature");
    assert_eq!(
        presented_kind("opaque-owner-or-legacy-credential"),
        "bearer"
    );
    assert_eq!(presented_kind("bad.payload"), "bearer");
    assert_eq!(presented_kind(""), "missing");
}
