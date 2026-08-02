use super::{encode_remote_tab_component, route_remote_tab_action, validate_remote_stream_owner};

#[test]
fn remote_tab_action_routes_to_composite_identity_owner_only() {
    for (method, suffix) in [
        ("GET", "/inspect"),
        ("POST", "/focus"),
        ("GET", "/output"),
        ("POST", "/send"),
        ("GET", "/chat"),
        ("POST", "/close"),
        ("POST", "/rename"),
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

#[test]
fn remote_route_components_are_url_safe() {
    assert_eq!(encode_remote_tab_component("таб:one"), "%D1%82%D0%B0%D0%B1%3Aone");
    let route = route_remote_tab_action("mesh:peer/таб:one", "peer", true, "POST", "/focus").unwrap();
    assert_eq!(route.path, "/api/tabs/%D1%82%D0%B0%D0%B1%3Aone/focus");
}

#[test]
fn remote_tab_stream_rejects_peer_global_and_mismatched_owner_routes() {
    let owner = validate_remote_stream_owner("peer-a", "mesh:peer-a/shared-tab", "/api/output/stream").unwrap();
    assert_eq!(owner.tab_id, "shared-tab");
    assert!(validate_remote_stream_owner("peer-b", "mesh:peer-a/shared-tab", "/api/output/stream").is_err());
    assert!(validate_remote_stream_owner("peer-a", "shared-tab", "/api/output/stream").is_err());
    assert!(validate_remote_stream_owner("peer-a", "mesh:peer-a/shared-tab", "/api/tabs").is_err());
}
