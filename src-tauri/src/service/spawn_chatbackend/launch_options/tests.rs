use super::*;

#[test]
fn explicit_host_false_cannot_be_broadened_by_an_available_authorization() {
    assert!(!permission_request(Some(false), true));
    assert!(!permission_request(Some(false), false));
}

#[test]
fn permission_bypass_requires_authorization_and_defaults_only_when_unspecified() {
    assert!(permission_request(Some(true), true));
    assert!(!permission_request(Some(true), false));
    assert!(permission_request(None, true));
    assert!(!permission_request(None, false));
}
