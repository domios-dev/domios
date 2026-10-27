use super::*;

#[test]
fn interactive_content_requires_an_explicit_owner_permission() {
    let mut permission: Permission = serde_json::from_value(
        serde_json::json!({"allowed":true,"scope":"opaque","allow_tab_requests":false}),
    )
    .unwrap();
    assert!(!permission.allow_interactive_content);
    permission.allow_interactive_content = true;
    let restored: Permission =
        serde_json::from_value(serde_json::to_value(permission).unwrap()).unwrap();
    assert!(restored.allow_interactive_content);
}

#[test]
fn scope_boundary_is_measured_in_utf8_bytes() {
    let mut permission = Permission {
        allowed: true,
        scope: "é".repeat(4096),
        allow_tab_requests: false,
        allow_interactive_content: false,
    };
    assert!(permission.validate().is_ok());
    assert!(permission.hint(Kind::Question).contains(&permission.scope));
    permission.scope.push('x');
    assert!(permission.validate().is_err());
}

#[test]
fn deny_by_default_and_tab_requests_need_an_explicit_grant() {
    let denied = Permission::default();
    assert!(!denied.allows(Kind::Question));
    let mut granted = Permission {
        allowed: true,
        scope: "Public project status".into(),
        allow_tab_requests: false,
        allow_interactive_content: false,
    };
    assert!(granted.allows(Kind::Question));
    assert!(!granted.allows(Kind::TabRequest));
    granted.allow_tab_requests = true;
    assert!(granted.allows(Kind::TabRequest));
    assert_ne!(
        denied.scope_sha256().unwrap(),
        granted.scope_sha256().unwrap()
    );
    granted.scope.clear();
    assert!(granted.validate().is_err());
}
