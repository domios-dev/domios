use super::*;

#[test]
fn local_only_paths_are_denied_at_every_peer_access_level() {
    for path in [
        "/api/secrets/opaque/value",
        "/api/mesh/debug/sign",
        "/api/supporter/token",
        "/api/analytics/usage",
        "/api/agents/reparent",
        "/api/teams/team/adopt-remote",
        "/api/account/status",
        "/api/account/logout",
        "/api/cli/rpc",
        "/api/model-providers",
        "/api/mesh/link",
        "/api/mesh/disconnect",
        "/api/mesh/unrevoke",
        "/api/mesh/peers/disconnect/peer",
        "/api/blueprints/graph/webhook/configure",
    ] {
        for suffix in ["", "?source=peer", "#fragment"] {
            let target = format!("{path}{suffix}");
            assert!(is_local_only_path(&target), "{target}");
            for access in [
                AccessLevel::Full,
                AccessLevel::ReadOnly,
                AccessLevel::AgentIsolated,
                AccessLevel::Custom(vec![Permission::Admin]),
            ] {
                for method in [Method::GET, Method::POST, Method::DELETE] {
                    assert!(!peer_access_allowed(
                        &access,
                        "peer",
                        &method,
                        &target,
                        |_| panic!("local-only paths do not resolve tab ownership"),
                    ));
                }
            }
        }
    }
    assert!(!is_local_only_path("/api/tabs/owned/text"));
    assert!(!is_local_only_path("/api/supporter/status"));
}

#[test]
fn local_preview_routes_follow_read_permissions_and_isolated_tab_ownership() {
    for suffix in ["read", "check"] {
        let path = format!("/api/tabs/owned/preview/{suffix}");
        assert!(check_access(&AccessLevel::ReadOnly, &Method::POST, &path));
        assert!(check_access(
            &AccessLevel::Custom(vec![Permission::ReadOutput]),
            &Method::POST,
            &path
        ));
        assert!(!check_access(
            &AccessLevel::Custom(vec![Permission::WriteTabs]),
            &Method::POST,
            &path
        ));
        assert!(peer_access_allowed(
            &AccessLevel::AgentIsolated,
            "peer-a",
            &Method::POST,
            &path,
            |_| Some("peer-a".into())
        ));
        assert!(!peer_access_allowed(
            &AccessLevel::AgentIsolated,
            "peer-a",
            &Method::POST,
            &path,
            |_| Some("peer-b".into())
        ));
    }
}

#[test]
fn read_only_allows_reads_and_mesh_control_only() {
    let access = AccessLevel::ReadOnly;
    assert!(check_access(&access, &Method::GET, "/api/tabs"));
    assert!(!check_access(&access, &Method::POST, "/api/mesh/announce"));
    assert!(check_access(&access, &Method::POST, "/api/mesh/sync"));
    assert!(!check_access(&access, &Method::POST, "/api/tabs/abc/send"));
    assert!(!check_access(&access, &Method::PUT, "/api/config"));
    assert!(!check_access(&access, &Method::PATCH, "/api/config"));
    assert!(!check_access(&access, &Method::DELETE, "/api/crons/123"));
}

#[test]
fn agent_isolated_keeps_agent_scope() {
    let access = AccessLevel::AgentIsolated;
    assert!(check_access(&access, &Method::GET, "/api/tabs"));
    assert!(check_access(&access, &Method::GET, "/api/tabs/abc/text"));
    assert!(check_access(&access, &Method::POST, "/api/tabs/abc/send"));
    assert!(check_access(&access, &Method::POST, "/api/tabs/abc/exec"));
    assert!(check_access(
        &access,
        &Method::POST,
        "/api/tabs/abc/respond"
    ));
    assert!(check_access(&access, &Method::POST, "/api/tabs/abc/report"));
    assert!(check_access(&access, &Method::POST, "/api/tabs/abc/clear"));
    assert!(check_access(&access, &Method::POST, "/api/agents/spawn"));
    assert!(!check_access(&access, &Method::GET, "/api/config"));
    assert!(!check_access(&access, &Method::GET, "/api/mem/search"));
    assert!(!check_access(&access, &Method::POST, "/api/mesh/announce"));
    assert!(check_access(&access, &Method::POST, "/api/mesh/sync"));
    assert!(!check_access(&access, &Method::POST, "/api/batch"));
    assert!(!check_access(&access, &Method::POST, "/api/config"));
    assert!(!check_access(&access, &Method::DELETE, "/api/crons/123"));
}

fn owned_by(access: &AccessLevel, method: &Method, path: &str, owner: Option<&str>) -> bool {
    peer_access_allowed(access, "peer-a", method, path, |_| {
        owner.map(str::to_string)
    })
}

#[test]
fn query_string_does_not_change_the_access_decision() {
    let levels = [
        AccessLevel::Full,
        AccessLevel::ReadOnly,
        AccessLevel::AgentIsolated,
        AccessLevel::Custom(vec![Permission::ReadTabs, Permission::SendInput]),
    ];
    let requests = [
        (Method::GET, "/api/tabs"),
        (Method::GET, "/api/tabs/abc/text"),
        (Method::POST, "/api/tabs/abc/send"),
        (Method::POST, "/api/agents/spawn"),
        (Method::POST, "/api/mesh/announce"),
        (Method::GET, "/api/config"),
        (Method::GET, "/api/secrets/items"),
    ];
    for access in &levels {
        for (method, path) in &requests {
            let with_query = format!("{path}?agents=false&formatted=false");
            for owner in [Some("peer-a"), Some("peer-b"), None] {
                assert_eq!(
                    owned_by(access, method, path, owner),
                    owned_by(access, method, &with_query, owner),
                    "{access:?} {method} {with_query}"
                );
            }
        }
    }
    let isolated = AccessLevel::AgentIsolated;
    let listing = "/api/tabs?agents=false";
    assert!(owned_by(&isolated, &Method::GET, listing, None));
    let own_text = "/api/tabs/abc/text?formatted=false";
    assert!(owned_by(&isolated, &Method::GET, own_text, Some("peer-a")));
}

#[test]
fn agent_isolated_owner_lookup_targets_the_path_tab() {
    let isolated = AccessLevel::AgentIsolated;
    for (method, path) in [
        (Method::POST, "/api/tabs/foreign/send"),
        (Method::POST, "/api/tabs/foreign/exec"),
        (Method::POST, "/api/tabs/foreign/respond"),
        (Method::POST, "/api/tabs/foreign/prompt/respond"),
        (Method::POST, "/api/tabs/foreign/attachments"),
        (Method::GET, "/api/tabs/foreign/text?formatted=false"),
    ] {
        let mut asked = None;
        let allowed = peer_access_allowed(&isolated, "peer-a", &method, path, |tab| {
            asked = Some(tab.to_string());
            Some("peer-b".to_string())
        });
        assert!(!allowed, "{method} {path}");
        assert_eq!(asked.as_deref(), Some("foreign"), "{method} {path}");
        assert!(!owned_by(&isolated, &method, path, None));
        assert!(owned_by(&isolated, &method, path, Some("peer-a")));
    }
}

#[test]
fn full_peer_access_never_consults_tab_ownership() {
    let allowed = peer_access_allowed(
        &AccessLevel::Full,
        "peer-a",
        &Method::POST,
        "/api/tabs/foreign/send",
        |_| panic!("owner lookup is AgentIsolated-only"),
    );
    assert!(allowed);
}

#[test]
fn custom_permissions_remain_explicit() {
    let access = AccessLevel::Custom(vec![Permission::ReadTabs, Permission::SendInput]);
    assert!(check_access(&access, &Method::GET, "/api/tabs"));
    assert!(check_access(&access, &Method::POST, "/api/tabs/abc/send"));
    assert!(!check_access(&access, &Method::POST, "/api/agents/spawn"));
    assert!(!check_access(&access, &Method::POST, "/api/config"));
}

#[test]
fn peer_local_secrets_are_never_authorized() {
    for access in [
        AccessLevel::Full,
        AccessLevel::ReadOnly,
        AccessLevel::AgentIsolated,
        AccessLevel::Custom(vec![Permission::Admin]),
    ] {
        assert!(!check_access(&access, &Method::GET, "/api/secrets/items"));
        assert!(!check_access(&access, &Method::GET, "/api/mesh/debug/sign"));
    }
}

#[test]
fn supporter_token_route_denied_to_peers() {
    for access in [
        AccessLevel::Full,
        AccessLevel::ReadOnly,
        AccessLevel::AgentIsolated,
        AccessLevel::Custom(vec![Permission::Admin]),
    ] {
        for method in [Method::GET, Method::POST, Method::DELETE] {
            assert!(!check_access(&access, &method, SUPPORTER_TOKEN_PATH));
            assert!(!check_access(
                &access,
                &method,
                "/api/supporter/token?adapter=x"
            ));
        }
    }
    assert!(check_access(
        &AccessLevel::Full,
        &Method::GET,
        "/api/supporter/status"
    ));
}

#[test]
fn mesh_control_denied_below_full() {
    for access in [AccessLevel::ReadOnly, AccessLevel::AgentIsolated] {
        assert!(!check_access(&access, &Method::POST, "/api/mesh/announce"));
        assert!(check_access(&access, &Method::POST, "/api/mesh/sync"));
    }
    assert!(check_access(
        &AccessLevel::Full,
        &Method::POST,
        "/api/mesh/announce"
    ));
}
