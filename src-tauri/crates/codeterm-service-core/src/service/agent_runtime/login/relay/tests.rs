use super::*;
#[test]
fn phone_payload_preserves_opaque_values_and_refuses_expired_login() {
    let pending:RuntimeLoginState=serde_json::from_value(json!({"status":"pending","url":"https://example.invalid/opaque-path?x=2&y=3","userCode":"opaque-token","expiresAtMs":5000})).unwrap();
    assert_eq!(phone_body(&pending).unwrap(),"https://example.invalid/opaque-path?x=2&y=3\nDevice code: opaque-token\nExpires at (Unix milliseconds): 5000");
    assert!(phone_body(&project(Some(&pending), 5000).unwrap()).is_err());
    assert!(phone_body(&RuntimeLoginState {
        url: None,
        ..pending
    })
    .is_err());
}
