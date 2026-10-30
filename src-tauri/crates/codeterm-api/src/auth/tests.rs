use super::*;
use codeterm_service_core::api_support::access_policy::check_access;

mod credentials;

#[test]
fn tailnet_webhooks_reach_handler_authentication_only_on_exact_post_routes() {
    for path in [
        crate::inbound_webhook::ci::PATH,
        "/api/blueprints/graph/webhook",
        codeterm_service_core::service::web_access::PEBBLE_WEBHOOK_PATH,
    ] {
        assert!(listener_bearer_exempt(&Method::POST, path, true), "{path}");
        assert!(listener_bearer_exempt(&Method::POST, path, false), "{path}");
        assert!(!tailnet_exempt(&Method::POST, path), "{path}");
        for method in [Method::GET, Method::PUT, Method::DELETE] {
            assert!(
                !listener_bearer_exempt(&method, path, true),
                "{method} {path}"
            );
        }
        assert!(!listener_bearer_exempt(
            &Method::POST,
            &format!("{path}/extra"),
            true,
        ));
    }
    for path in [
        "/api/blueprints/graph/webhook/configure",
        "/api/blueprints/graph/extra/webhook",
        "/api/ci/webhook/extra",
        "/api/graph/events/graph",
        "/api/mesh/relay-asset/peer/foo",
        "/api/secrets/list",
    ] {
        assert!(!listener_bearer_exempt(&Method::POST, path, true), "{path}");
    }
}

#[test]
fn tailnet_exemptions_are_only_existing_peer_handshakes_and_liveness() {
    for path in ["/api/health", "/api/mesh/ping"] {
        assert!(tailnet_exempt(&Method::GET, path));
        assert!(!tailnet_exempt(&Method::POST, path));
    }
    for path in [
        "/api/mesh/link/hello",
        "/api/mesh/link/confirm",
        "/api/mesh/link/commit",
        "/api/mesh/relay",
        "/api/mesh/announce",
        "/api/mesh/account-presence",
    ] {
        assert!(tailnet_exempt(&Method::POST, path));
        assert!(!tailnet_exempt(&Method::GET, path));
    }
    for path in [
        "/",
        "/frontend/index.html",
        "/api/config",
        "/api/secrets/list",
        "/api/mesh/relay-asset/peer/foo",
        "/api/tabs/tab/hook/stop",
    ] {
        assert!(!tailnet_exempt(&Method::GET, path));
        assert!(!tailnet_exempt(&Method::POST, path));
    }
}

#[tokio::test]
async fn tailnet_router_accepts_instance_tokens_and_preserves_peer_access_policy() {
    use axum::{body::Body, http::Request, routing::get, Router};
    use codeterm_service_core::service::account_mesh::tailnet::TailnetListener;
    use codeterm_service_core::{
        mesh::account_mesh::{AccountMeshState, AccountPresence},
        mesh::types::PairedPeer,
        multiplexer::state::MultiplexerState,
        service::test_fixture::ServiceFixture,
    };
    use tower::ServiceExt;

    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    assert!(std::sync::Arc::ptr_eq(
        &state.mesh,
        fixture
            .service
            .ext::<codeterm_service_core::MeshStateHandle>(),
    ));
    let (private_key, public_key) = codeterm_service_core::mesh::crown::generate_peer_keypair();
    {
        let mut mesh = state.mesh.lock();
        mesh.insert_account_peer_for_test(PairedPeer {
            instance_id: "paired".into(),
            instance_name: "paired".into(),
            addresses: vec![],
            api_port: 7685,
            public_key,
            paired_at: "now".into(),
            last_seen: None,
            online: true,
            is_primary: false,
            access_level: AccessLevel::ReadOnly,
            version: None,
            web_ui: None,
            relay_protocol_version: codeterm_service_core::mesh::types::RELAY_PROTOCOL_VERSION,
            consecutive_misses: 0,
            last_active_ms: None,
        });
        mesh.apply_account_state(AccountMeshState {
            presence: AccountPresence::SignedIn {
                account_id: "account".into(),
            },
            ..Default::default()
        });
    }
    let router = Router::new()
        .route(
            "/api/config",
            get(|| async { axum::Json(serde_json::json!({"backing": {"plugin_id": "browser", "surface_id": "surface", "resume_url": "private"}})) })
                .post(|| async { "written" }),
        )
        .route("/api/health", get(|| async { "healthy" }))
        .route("/api/secrets/list", get(|| async { "secret" }))
        .route("/frontend/index.html", get(|| async { "frontend" }))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .with_state(state.clone());
    let signed = codeterm_service_core::mesh::crown::sign_request(&private_key, "paired").unwrap();
    let request = |method: &str, path: &str, token: &str| {
        let mut req = Request::builder()
            .method(method)
            .uri(path)
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(TailnetListener);
        req
    };
    for token in ["", "forged"] {
        assert_eq!(
            router
                .clone()
                .oneshot(request("GET", "/api/config", token))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    let response = router
        .clone()
        .oneshot(request("GET", "/api/config", &signed))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("private"));
    assert_eq!(
        router
            .clone()
            .oneshot(request("POST", "/api/config", &signed))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        router
            .clone()
            .oneshot(request("GET", "/api/secrets/list", &signed))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        router
            .clone()
            .oneshot(request("GET", "/frontend/index.html", &signed))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        router
            .clone()
            .oneshot(request("GET", "/api/health", ""))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    for presence in [AccountPresence::NeverSignedIn, AccountPresence::SignedOut] {
        state.mesh.lock().apply_account_state(AccountMeshState {
            presence,
            ..Default::default()
        });
        for (token, expected) in [
            ("preview-test-token", StatusCode::OK),
            ("", StatusCode::UNAUTHORIZED),
            (signed.as_str(), StatusCode::UNAUTHORIZED),
        ] {
            assert_eq!(
                router
                    .clone()
                    .oneshot(request("GET", "/api/config", token))
                    .await
                    .unwrap()
                    .status(),
                expected
            );
        }
        assert_eq!(
            router
                .clone()
                .oneshot(request("GET", "/api/health", ""))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
    }

    state.mesh.lock().apply_account_state(AccountMeshState {
        presence: AccountPresence::SignedIn {
            account_id: "account".into(),
        },
        ..Default::default()
    });
    state.mesh.lock().remove_account_peer("paired");
    assert_eq!(
        router
            .clone()
            .oneshot(request("GET", "/api/config", &signed))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        router
            .oneshot(request("GET", "/api/config", "preview-test-token"))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
}

#[test]
fn remote_ownership_release_accepts_only_local_identity() {
    assert!(ownership_release_is_local(CallerIdentity::Local));
    assert!(!ownership_release_is_local(CallerIdentity::Peer {
        instance_id: "peer-eight".into(),
        access_level: AccessLevel::Full,
        relay_protocol_version: codeterm_service_core::mesh::types::RELAY_PROTOCOL_VERSION,
    }));
}

#[test]
fn hook_auth_context_only_matches_binding_events() {
    assert_eq!(
        tab_hook_target(&Method::POST, "/api/tabs/tab-1/hook/session_start"),
        Some(("tab-1", "session_start")),
    );
    assert_eq!(
        tab_hook_target(&Method::POST, "/api/tabs/tab-1/hook/provider_event"),
        Some(("tab-1", "provider_event")),
    );
    assert_eq!(
        tab_hook_target(&Method::POST, "/api/tabs/tab-1/hook/stop"),
        Some(("tab-1", "stop")),
    );
    assert_eq!(
        tab_hook_target(&Method::GET, "/api/tabs/tab-1/hook/stop"),
        None,
    );
    assert_eq!(
        tab_hook_target(&Method::POST, "/api/tabs/tab-1/hook/permission"),
        None,
    );
}

#[test]
fn plugin_ui_document_and_assets_require_bearer_authentication() {
    for path in [
        "/api/plugins/example/ui/index.html",
        "/api/plugins/example/ui/app.js",
        "/api/plugins/example/ui/icon.svg",
    ] {
        assert!(!bearer_exempt(&Method::GET, path));
    }
    assert!(bearer_exempt(&Method::GET, "/api/health"));
    assert!(bearer_exempt(&Method::GET, "/frontend/index.html"));
}

#[test]
fn custom_auth_only_exempts_self_verifying_paths() {
    // Self-verifying: skip the bearer middleware.
    assert!(is_custom_auth(&Method::POST, "/api/mesh/relay"));
    assert!(!is_custom_auth(&Method::POST, "/api/mesh/adopt/offer"));
    assert!(!is_custom_auth(&Method::POST, "/api/mesh/adopt/commit"));
    assert!(!is_custom_auth(&Method::POST, "/api/mesh/introduce"));
    assert!(!is_custom_auth(&Method::DELETE, "/api/mesh/peers/abc123")); // unpair
                                                                         // Relayed plugin-UI asset proxy: iframe src, handler self-verifies query token.
    assert!(is_custom_auth(
        &Method::GET,
        "/api/mesh/relay-asset/peer1/plugins/foo/ui/index.html"
    ));
    // The bare route without a peer segment is not the proxy — not exempt.
    assert!(!is_custom_auth(&Method::GET, "/api/mesh/relay-asset"));

    // therefore fully unauthenticated. They must NOT be custom-auth.
    assert!(!is_custom_auth(
        &Method::GET,
        "/api/mesh/peers/abc123/services"
    ));
    assert!(!is_custom_auth(&Method::POST, "/api/mesh/peers/add-manual"));
    assert!(!is_custom_auth(
        &Method::POST,
        "/api/mesh/peers/approve/abc123"
    ));
    assert!(!is_custom_auth(
        &Method::POST,
        "/api/mesh/peers/reject/abc123"
    ));
    assert!(!is_custom_auth(
        &Method::POST,
        "/api/mesh/peers/abc123/addr"
    ));
    // Unpair must be DELETE — a GET on the same path is not exempt.
    assert!(!is_custom_auth(&Method::GET, "/api/mesh/peers/abc123"));

    assert!(is_inbound_webhook_path(
        &Method::POST,
        "/api/plugins/pebble/webhook"
    ));
    assert!(is_custom_auth(&Method::POST, "/api/plugins/pebble/webhook"));
    assert!(!is_inbound_webhook_path(
        &Method::GET,
        "/api/plugins/pebble/webhook"
    ));
    assert!(!is_inbound_webhook_path(
        &Method::POST,
        "/api/plugins/pebble/webhook/extra"
    ));
    assert!(!is_inbound_webhook_path(
        &Method::POST,
        "/api/plugins/pebble/ui/webhook"
    ));
    assert!(!is_inbound_webhook_path(
        &Method::POST,
        "/api/plugins/other/webhook"
    ));
    assert!(!is_custom_auth(&Method::POST, "/api/plugins/other/webhook"));
    assert!(!is_custom_auth(
        &Method::POST,
        "/api/plugins/pebble/webhook/extra"
    ));
}

#[test]
fn v1_14_9_peer_protocol_is_refused_with_a_version_upgrade_message() {
    crate::test_support::install_app_version();
    let ancient_protocol = codeterm_service_core::mesh::types::RELAY_PROTOCOL_VERSION - 1;
    let refusal = peer_protocol_refusal(ancient_protocol).unwrap();
    assert!(refusal.contains(&format!(
        "both peers on Domios {}",
        crate::test_support::TEST_APP_VERSION
    )));
    assert!(refusal.contains("upgrade both peers through the updater"));
    for path in ["/api/mesh/announce", "/api/mesh/sync"] {
        assert!(!peer_protocol_allowed(ancient_protocol, path));
    }
    assert!(!peer_protocol_allowed(
        ancient_protocol,
        "/api/events/stream"
    ));
    assert!(!peer_protocol_allowed(ancient_protocol, "/api/config"));
    assert!(!peer_protocol_allowed(
        ancient_protocol,
        "/api/tabs/abc/send"
    ));

    for version in codeterm_service_core::mesh::types::RELAY_PROTOCOL_VERSION
        ..=codeterm_service_core::mesh::types::MAX_SUPPORTED_RELAY_PROTOCOL_VERSION
    {
        assert!(peer_protocol_allowed(version, "/api/mesh/announce"));
        assert!(peer_protocol_allowed(version, "/api/events/stream"));
        assert!(peer_protocol_allowed(version, "/api/tabs/abc/send"));
    }
}

#[test]
fn v1_14_peer_gets_empty_gossip_on_sync_and_upgrade_required_elsewhere() {
    crate::test_support::install_app_version();
    let ancient = codeterm_service_core::mesh::types::RELAY_PROTOCOL_VERSION - 1;
    let current = codeterm_service_core::mesh::types::RELAY_PROTOCOL_VERSION;

    assert_eq!(
        stored_protocol_answer(&Method::POST, "/api/mesh/sync", ancient),
        StoredProtocolAnswer::EmptyGossip
    );
    assert!(matches!(
        stored_protocol_answer(&Method::GET, "/api/mesh/inventory", ancient),
        StoredProtocolAnswer::UpgradeRequired(_)
    ));
    assert!(matches!(
        stored_protocol_answer(&Method::GET, "/api/mesh/sync", ancient),
        StoredProtocolAnswer::UpgradeRequired(_)
    ));
    assert_eq!(
        stored_protocol_answer(&Method::POST, "/api/mesh/announce", ancient),
        StoredProtocolAnswer::Proceed
    );
    for (method, path) in [
        (Method::POST, "/api/mesh/sync"),
        (Method::GET, "/api/mesh/inventory"),
    ] {
        assert_eq!(
            stored_protocol_answer(&method, path, current),
            StoredProtocolAnswer::Proceed
        );
    }
    assert_eq!(
        serde_json::to_value(empty_gossip_snapshot()).unwrap(),
        serde_json::json!({ "peers": [], "generation": 0 })
    );
}

#[test]
fn upgraded_peer_announce_refreshes_a_stored_v1_record_then_other_routes_pass() {
    use codeterm_service_core::mesh::types::{
        PairedPeer, PeerAddress, PeerAnnounce, RELAY_PROTOCOL_VERSION,
    };
    let mut mesh = codeterm_service_core::mesh::MeshManager::new("auth-test".into(), 7685);
    mesh.disk_persist = false;
    mesh.insert_account_peer_for_test(PairedPeer {
        instance_id: "air".into(),
        instance_name: "air".into(),
        addresses: vec![PeerAddress::from_ip("100.64.0.7")],
        api_port: 7685,
        public_key: String::new(),
        paired_at: "then".into(),
        last_seen: None,
        online: true,
        is_primary: false,
        access_level: AccessLevel::Full,
        version: None,
        web_ui: None,
        relay_protocol_version: 1,
        consecutive_misses: 0,
        last_active_ms: None,
    });
    let stored = |mesh: &codeterm_service_core::mesh::MeshManager| {
        mesh.paired_peers["air"].relay_protocol_version
    };
    let announce = |relay_protocol_version| PeerAnnounce {
        registry: None,
        instance_id: "air".into(),
        instance_name: "air".into(),
        addresses: vec![PeerAddress::from_ip("100.64.0.7")],
        api_port: 7685,
        online: true,
        web_ui: None,
        relay_protocol_version,
    };

    assert!(stored_protocol_refusal(&Method::GET, "/api/mesh/inventory", stored(&mesh)).is_some());
    assert!(stored_protocol_refusal(&Method::POST, "/api/mesh/sync", stored(&mesh)).is_some());
    assert!(stored_protocol_refusal(&Method::POST, "/api/mesh/announce", stored(&mesh)).is_none());

    assert!(mesh.apply_announce("air", &announce(1)).is_err());
    assert_eq!(stored(&mesh), 1);

    mesh.apply_announce("air", &announce(RELAY_PROTOCOL_VERSION))
        .expect("v2 announce accepted over a stored v1 record");
    assert_eq!(stored(&mesh), RELAY_PROTOCOL_VERSION);
    assert!(stored_protocol_refusal(&Method::GET, "/api/mesh/inventory", stored(&mesh)).is_none());
    assert!(stored_protocol_refusal(&Method::POST, "/api/mesh/sync", stored(&mesh)).is_none());
}

#[test]
fn only_the_announce_post_bypasses_the_stored_protocol() {
    assert!(stored_protocol_refusal(&Method::GET, "/api/mesh/announce", 1).is_some());
    assert!(stored_protocol_refusal(&Method::POST, "/api/mesh/announce/all", 1).is_some());
    assert!(stored_protocol_refusal(&Method::POST, "/api/tabs/abc/send", 1).is_some());
}

#[test]
fn full_access_allows_everything() {
    assert!(check_access(
        &AccessLevel::Full,
        &Method::GET,
        "/api/config"
    ));
    assert!(check_access(
        &AccessLevel::Full,
        &Method::POST,
        "/api/tabs/abc/send"
    ));
    assert!(check_access(
        &AccessLevel::Full,
        &Method::DELETE,
        "/api/crons/123"
    ));
}

#[test]
fn read_only_blocks_writes() {
    assert!(check_access(
        &AccessLevel::ReadOnly,
        &Method::GET,
        "/api/tabs"
    ));
    assert!(check_access(
        &AccessLevel::ReadOnly,
        &Method::GET,
        "/api/config"
    ));
    assert!(!check_access(
        &AccessLevel::ReadOnly,
        &Method::POST,
        "/api/tabs/abc/send"
    ));
    assert!(!check_access(
        &AccessLevel::ReadOnly,
        &Method::DELETE,
        "/api/crons/123"
    ));
}

#[test]
fn agent_isolated_allows_agent_ops() {
    let level = AccessLevel::AgentIsolated;
    assert!(check_access(&level, &Method::GET, "/api/tabs"));
    assert!(check_access(&level, &Method::GET, "/api/tabs/abc/text"));
    assert!(check_access(&level, &Method::POST, "/api/tabs/abc/send"));
    assert!(check_access(&level, &Method::POST, "/api/tabs/abc/exec"));
    assert!(check_access(&level, &Method::POST, "/api/tabs/abc/respond"));
    assert!(check_access(&level, &Method::POST, "/api/agents/spawn"));
    assert!(check_access(&level, &Method::POST, "/api/tabs/abc/report"));
    assert!(check_access(&level, &Method::POST, "/api/tabs/abc/clear"));
    assert!(!check_access(&level, &Method::GET, "/api/config"));
    assert!(!check_access(&level, &Method::GET, "/api/mem/search"));
    assert!(!check_access(&level, &Method::GET, "/api/notifications"));
    assert!(!check_access(&level, &Method::GET, "/api/workspaces"));
    assert!(!check_access(&level, &Method::POST, "/api/config"));
    assert!(!check_access(&level, &Method::DELETE, "/api/crons/123"));
    assert!(!check_access(&level, &Method::POST, "/api/mesh/link"));
}

#[test]
fn agent_isolated_limited_to_spawned_tabs() {
    let (mut mux, _) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let tab_id = mux.snapshot().workspaces[0].tabs[0].clone();
    mux.get_tab_mut(&tab_id).unwrap().ownership = Some(codeterm_types::pty::RemoteOwnership {
        controller_title: "remote".into(),
        controller_tab_id: None,
        peer_id: "peer-a".into(),
        peer_name: "Peer A".into(),
        team: None,
    });
    let level = AccessLevel::AgentIsolated;
    for (method, path) in [
        (Method::GET, "/api/tabs/abc/text"),
        (Method::POST, "/api/tabs/abc/send"),
        (Method::POST, "/api/tabs/abc/exec"),
        (Method::POST, "/api/tabs/abc/prompt/respond"),
        (Method::POST, "/api/tabs/abc/report"),
        (Method::POST, "/api/tabs/abc/prompt/clear"),
        (Method::POST, "/api/tabs/abc/attachments"),
    ] {
        let owner_is = |owner: &'static str| {
            peer_access_allowed(&level, "peer-a", &method, path, |_| Some(owner.into()))
        };
        assert!(owner_is("peer-a"));
        assert!(!owner_is("peer-b"));
    }
    assert!(!peer_access_allowed(
        &level,
        "peer-a",
        &Method::POST,
        "/api/tabs/abc/send",
        |_| None
    ));
    assert!(peer_owns_tab("peer-a", mux.get_tab(&tab_id).unwrap()));
}

#[test]
fn custom_permissions() {
    use codeterm_service_core::mesh::types::Permission;

    let level = AccessLevel::Custom(vec![Permission::ReadTabs, Permission::SendInput]);
    assert!(check_access(&level, &Method::GET, "/api/tabs"));
    assert!(check_access(&level, &Method::POST, "/api/tabs/abc/send"));
    assert!(!check_access(&level, &Method::POST, "/api/agents/spawn"));
    assert!(!check_access(&level, &Method::POST, "/api/config"));

    let admin = AccessLevel::Custom(vec![Permission::Admin]);
    assert!(check_access(&admin, &Method::POST, "/api/config"));
    assert!(check_access(&admin, &Method::DELETE, "/api/crons/123"));
}

#[test]
fn public_paths_recognized() {
    for p in PUBLIC_PATHS {
        assert!(PUBLIC_PATHS.iter().any(|pp| pp == p));
    }
    assert!(!PUBLIC_PATHS.contains(&"/api/providers"));
    for removed in [
        "/api/mesh/connect",
        "/api/mesh/approve",
        "/api/mesh/peers/add-manual",
        "/api/mesh/pair/request",
        "/api/mesh/pair/identity",
        "/api/mesh/pair/accept",
        "/api/mesh/pair/confirm",
        "/api/mesh/pair/reject",
        "/api/mesh/link",
    ] {
        assert!(!PUBLIC_PATHS.contains(&removed), "{removed}");
    }
}

#[test]
fn remote_network_follows_mesh_toggle() {
    let local = "127.0.0.1:7685".parse().unwrap();
    let remote = "192.0.2.10:7685".parse().unwrap();

    assert!(remote_network_allowed(false, None));
    assert!(remote_network_allowed(false, Some(local)));
    assert!(!remote_network_allowed(false, Some(remote)));
    assert!(remote_network_allowed(true, Some(remote)));
}

#[test]
fn custom_auth_paths() {
    assert!(is_custom_auth(&Method::POST, "/api/mesh/relay"));
    assert!(!is_custom_auth(&Method::POST, "/api/mesh/link"));
    // Unpair self-verifies; only the DELETE on the bare id is exempt.
    assert!(!is_custom_auth(&Method::DELETE, "/api/mesh/peers/some-id"));
    assert!(!is_custom_auth(&Method::GET, "/api/mesh/peers/some-id"));
    assert!(!is_custom_auth(&Method::GET, "/api/mesh/peers"));
    assert!(!is_custom_auth(&Method::GET, "/api/mesh/status"));
    assert!(!is_custom_auth(&Method::POST, "/api/tabs/abc/send"));
}

#[test]
fn service_push_allowed_on_send() {
    assert!(is_service_push_allowed("POST", "/api/tabs/abc123/send"));
    assert!(is_service_push_allowed("POST", "/api/tabs/general/send"));
    assert!(is_service_push_allowed(
        "POST",
        "/api/tabs/tab-with-dashes_and_underscores/send"
    ));
}

#[test]
fn local_inventory_read_does_not_require_mesh_network() {
    assert!(!requires_mesh_network(&Method::GET, "/api/mesh/inventory"));
    assert!(requires_mesh_network(&Method::POST, "/api/mesh/inventory"));
    assert!(requires_mesh_network(&Method::GET, "/api/mesh/peers"));
    assert!(requires_mesh_network(
        &Method::GET,
        "/api/mesh/inventory/other"
    ));
    assert!(!bearer_exempt(&Method::GET, "/api/mesh/inventory"));
}

#[test]
fn service_push_rejected_on_config() {
    assert!(!is_service_push_allowed("POST", "/api/config"));
}

#[test]
fn service_push_rejected_on_spawn() {
    assert!(!is_service_push_allowed("POST", "/api/agents/spawn"));
}

#[test]
fn service_push_rejected_on_get() {
    assert!(!is_service_push_allowed("GET", "/api/tabs/abc123/send"));
    assert!(!is_service_push_allowed("DELETE", "/api/tabs/abc123/send"));
    assert!(!is_service_push_allowed("PUT", "/api/tabs/abc123/send"));
}

#[test]
fn service_push_rejected_on_tab_root() {
    assert!(!is_service_push_allowed("POST", "/api/tabs/abc123"));
    assert!(!is_service_push_allowed("POST", "/api/tabs/abc123/exec"));
    assert!(!is_service_push_allowed("POST", "/api/tabs/abc123/clear"));
    assert!(!is_service_push_allowed("POST", "/api/tabs//send"));
    assert!(!is_service_push_allowed("POST", "/api/tabs/abc/extra/send"));
    assert!(!is_service_push_allowed("POST", "/api/tabs"));
}

#[test]
fn migrated_instance_refuses_legacy_bearer_and_accepts_persisted_replacement() {
    use codeterm_service_core::multiplexer::state::MultiplexerState;
    use codeterm_service_core::service::test_fixture::ServiceFixture;

    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    let rotated =
        codeterm_util::instance_token::migrate_at(fixture.temp.path(), "legacy-instance-bearer")
            .unwrap();
    state.config.lock().api.token = Some(rotated.clone());
    assert!(identify_caller(&state, "Bearer legacy-instance-bearer").is_err());
    assert!(matches!(
        identify_caller(&state, &format!("Bearer {rotated}")),
        Ok(CallerIdentity::Local)
    ));
    assert_eq!(
        codeterm_util::instance_token::read_at(fixture.temp.path()).unwrap(),
        Some(rotated)
    );
}

#[test]
fn plan_limit_locked_peer_is_named_instead_of_an_unknown_signature() {
    use codeterm_service_core::mesh::account_mesh::{AccountMeshState, AccountPresence};
    use codeterm_service_core::mesh::types::PairedPeer;
    use codeterm_service_core::multiplexer::state::MultiplexerState;
    use codeterm_service_core::service::test_fixture::ServiceFixture;

    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    let (private_key, public_key) = codeterm_service_core::mesh::crown::generate_peer_keypair();
    let sign_in = |locked| AccountMeshState {
        account_epoch: 1 + u64::from(locked),
        presence: AccountPresence::SignedIn {
            account_id: "account".into(),
        },
        plan_limit_locked: locked,
        ..AccountMeshState::default()
    };
    {
        let mut mesh = state.mesh.lock();
        mesh.insert_account_peer_for_test(PairedPeer {
            instance_id: "locked-peer".into(),
            instance_name: "locked".into(),
            addresses: vec![],
            api_port: 0,
            public_key,
            paired_at: "now".into(),
            last_seen: None,
            online: true,
            is_primary: false,
            access_level: AccessLevel::Full,
            version: None,
            web_ui: None,
            relay_protocol_version: codeterm_service_core::mesh::types::RELAY_PROTOCOL_VERSION,
            consecutive_misses: 0,
            last_active_ms: None,
        });
        mesh.apply_account_state(sign_in(true));
    }
    let token =
        codeterm_service_core::mesh::crown::sign_request(&private_key, "locked-peer").unwrap();
    assert!(identify_caller(&state, &format!("Bearer {token}")).is_err());
    assert_eq!(
        plan_limit_locked_peer(&state, &token).as_deref(),
        Some("locked-peer")
    );
    assert_eq!(plan_limit_locked_peer(&state, "forged"), None);
    state.mesh.lock().apply_account_state(sign_in(false));
    assert!(identify_caller(&state, &format!("Bearer {token}")).is_ok());
    assert_eq!(plan_limit_locked_peer(&state, &token), None);
}

#[test]
fn presence_notice_auth_exemption_is_exact_and_post_only() {
    assert!(is_custom_auth(&Method::POST, "/api/mesh/account-presence"));
    for path in [
        "/api/mesh/account-presence/extra",
        "/api/mesh/account-presence-all",
    ] {
        assert!(!is_custom_auth(&Method::POST, path));
        assert!(!tailnet_exempt(&Method::POST, path));
    }
    assert!(!is_custom_auth(&Method::GET, "/api/mesh/account-presence"));
    assert!(!tailnet_exempt(&Method::GET, "/api/mesh/account-presence"));
}
