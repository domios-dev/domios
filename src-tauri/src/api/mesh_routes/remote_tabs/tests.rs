use super::route_remote_tab_action;

#[test]
fn remote_tab_action_routes_to_composite_identity_owner_only() {
    for (method, suffix) in [
        ("GET", "/inspect"),
        ("POST", "/focus"),
        ("GET", "/output"),
        ("POST", "/send"),
        ("GET", "/chat"),
    ] {
        let route = route_remote_tab_action(
            "mesh:peer-a/shared-tab",
            "peer-a",
            true,
            method,
            suffix,
        )
        .unwrap();
        assert_eq!(route.method, method);
        let expected = match suffix {
            "/inspect" => "/api/tabs/inspect?tab_ids=shared-tab".to_string(),
            _ => format!("/api/tabs/shared-tab{suffix}"),
        };
        assert_eq!(route.path, expected);
        assert_eq!(route.delivery_id.is_some(), suffix == "/send");
    }

    assert!(route_remote_tab_action("shared-tab", "peer-a", true, "GET", "/output").is_err());
    assert!(route_remote_tab_action("mesh:peer-a/shared-tab", "peer-b", true, "GET", "/output").is_err());
    assert!(route_remote_tab_action("mesh:peer-a/shared-tab", "peer-a", false, "GET", "/output").is_err());
}
