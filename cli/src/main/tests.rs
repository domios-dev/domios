use super::restricted_cli_scope;
use std::ffi::OsStr;

#[test]
fn scoped_cli_cannot_fall_back_to_owner_store_when_launch_credential_is_missing() {
    assert!(restricted_cli_scope(Some(OsStr::new("service-operations")), true).is_ok());
    assert!(restricted_cli_scope(Some(OsStr::new("service-operations")), false).is_err());
    for scope in ["identify-only", "unknown"] {
        assert!(restricted_cli_scope(Some(OsStr::new(scope)), true).is_err());
    }
    for inherited in [false, true] { assert!(restricted_cli_scope(None, inherited).is_ok()); }
}
