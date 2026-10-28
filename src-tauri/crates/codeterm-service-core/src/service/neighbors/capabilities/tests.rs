use super::*;

#[test]
fn default_deny_rejects_every_non_allowlisted_rpc_and_http_surface() {
    let permission = Permission {
        allowed: true,
        scope: "opaque".into(),
        allow_tab_requests: true,
        ..Default::default()
    };
    for method in [
        "mem.save",
        "mem.append",
        "mem.delete",
        "mem.secret.get",
        "tab.list",
        "tab.get_output",
        "tab.send_text",
        "tab.exec",
        "cron.list",
        "cron.update",
        "cron.trigger",
        "config.set",
        "model_provider.discover",
        "plugin:opaque.read",
        "bridge.send",
        "notification.create",
        "blueprint.apply_recipe",
        "blueprint.import",
        "mesh.relay",
        "unknown.new_method",
    ] {
        assert!(
            rpc_capability(method)
                .unwrap()
                .require(&permission)
                .unwrap_err()
                .starts_with("neighbor_capability_denied:"),
            "{method}"
        );
    }
    for (method, path) in [
        ("PATCH", "/api/config"),
        ("GET", "/api/tabs"),
        ("POST", "/api/tabs/foreign/send"),
        ("GET", "/api/secrets/item"),
        ("POST", "/api/mem/items"),
        ("POST", "/api/mesh/relay-to"),
        ("GET", "/api/mesh/relay-stream"),
        ("GET", "/api/mesh/relay-asset/opaque"),
        ("POST", "/api/bridge/send"),
        ("POST", "/api/notifications"),
        ("POST", "/api/plugins/opaque/execute"),
        ("GET", "/api/fs/list-dir"),
        ("GET", "/api/serve-file"),
        ("POST", "/api/account/neighbors"),
        ("GET", "/api/unknown"),
    ] {
        assert!(
            http_capability(method, path)
                .unwrap()
                .require(&permission)
                .is_err(),
            "{method} {path}"
        );
    }
}

#[test]
fn memory_and_files_require_separate_grants_and_spawn_cannot_escape_without_inheritance() {
    let mut permission = Permission {
        allowed: true,
        scope: "opaque".into(),
        ..Default::default()
    };
    assert!(Capability::MemoryRead.require(&permission).is_ok());
    assert!(Capability::Files.require(&permission).is_ok());
    assert!(Capability::Spawn
        .require(&permission)
        .unwrap_err()
        .starts_with("tab_requests_not_allowed:"));
    permission.allow_tab_requests = true;
    assert!(Capability::Spawn
        .require(&permission)
        .unwrap_err()
        .starts_with("neighbor_capability_denied:"));
    permission.allow_memory = false;
    permission.allow_files = false;
    assert!(Capability::MemoryRead
        .require(&permission)
        .unwrap_err()
        .starts_with("neighbor_memory_not_allowed:"));
    assert!(Capability::Files
        .require(&permission)
        .unwrap_err()
        .starts_with("neighbor_files_not_allowed:"));
    for capability in [Capability::Neighbor, Capability::Report, Capability::Answer] {
        assert!(capability.require(&permission).is_ok());
    }
}

#[test]
fn allowlist_classifies_only_narrow_read_reply_and_neighbor_operations() {
    for method in ["mem.search", "mem.get", "mem.wake_up"] {
        assert!(matches!(
            rpc_capability(method),
            Some(Capability::MemoryRead)
        ));
    }
    assert!(matches!(
        rpc_capability("agent.report"),
        Some(Capability::Report)
    ));
    assert!(matches!(
        rpc_capability("api.request"),
        Some(Capability::Route)
    ));
    for path in ["/api/mem/search", "/api/mem/items/own", "/api/mem/wake-up"] {
        assert!(matches!(
            http_capability("GET", path),
            Some(Capability::MemoryRead)
        ));
        assert!(matches!(
            http_capability("POST", path),
            Some(Capability::Denied)
        ));
    }
}
#[tokio::test]
async fn capability_lookup_requires_the_exact_live_bound_concierge_nonce() {
    let (mut mux, general) = crate::multiplexer::state::MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let concierge = mux.create_tab(&workspace).unwrap();
    let unrelated = mux.create_tab(&workspace).unwrap();
    let tab = mux.get_tab_mut(&concierge).unwrap();
    tab.agent_type = Some("fixture".into());
    tab.agent_session_id = Some("own-session".into());
    tab.session_binding_nonce = Some("own-nonce".into());
    tab.reported_session_binding_nonce = Some("own-nonce".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friendship = "a".repeat(64);
    let mut settings = super::super::storage::Settings::default();
    settings
        .bindings
        .insert(friendship.clone(), concierge.clone());
    settings.permissions.insert(
        friendship.clone(),
        Permission {
            allowed: true,
            scope: "opaque".into(),
            ..Default::default()
        },
    );
    metadata.save_settings(settings).await.unwrap();
    let mut caller = TabCaller {
        tab_id: concierge,
        binding_nonce: "own-nonce".into(),
    };
    let bound = fixture
        .service
        .neighbor_capability_permission_at(&metadata, &caller)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(bound.0, friendship);
    assert!(Capability::MemoryRead.require(&bound.1).is_ok());
    let stored = metadata.settings().await.unwrap();
    fixture
        .service
        .remember_neighbor_concierge_accounts("owner", &stored);
    fixture
        .service
        .remember_neighbor_concierge_accounts("owner", &Default::default());
    assert!(fixture
        .service
        .authorize_neighbor_capability(&caller, Capability::MemoryRead)
        .await
        .is_err());
    caller.binding_nonce = "stale".into();
    assert!(fixture
        .service
        .neighbor_capability_permission_at(&metadata, &caller)
        .await
        .is_err());
    caller.tab_id = unrelated;
    assert!(fixture
        .service
        .neighbor_capability_permission_at(&metadata, &caller)
        .await
        .unwrap()
        .is_none());
}
