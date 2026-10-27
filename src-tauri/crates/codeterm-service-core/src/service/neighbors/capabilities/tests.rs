use super::*;

#[test]
fn concierge_memory_is_read_only_and_can_be_disabled_without_allowing_secrets() {
    let mut permission = Permission {
        allowed: true,
        scope: "opaque".into(),
        ..Default::default()
    };
    assert!(Capability::MemoryRead.require(&permission).is_ok());
    assert!(Capability::MemoryWrite
        .require(&permission)
        .unwrap_err()
        .starts_with("neighbor_memory_read_only:"));
    assert!(Capability::Secrets
        .require(&permission)
        .unwrap_err()
        .starts_with("neighbor_secrets_not_allowed:"));
    permission.allow_memory = false;
    assert!(Capability::MemoryRead
        .require(&permission)
        .unwrap_err()
        .starts_with("neighbor_memory_not_allowed:"));
}

#[test]
fn concierge_spawn_and_file_access_require_their_own_grants() {
    let mut permission = Permission {
        allowed: true,
        scope: "opaque".into(),
        ..Default::default()
    };
    assert!(Capability::Spawn
        .require(&permission)
        .unwrap_err()
        .starts_with("tab_requests_not_allowed:"));
    assert!(Capability::Files.require(&permission).is_ok());
    permission.allow_tab_requests = true;
    permission.allow_files = false;
    assert!(Capability::Spawn.require(&permission).is_ok());
    assert!(Capability::Files
        .require(&permission)
        .unwrap_err()
        .starts_with("neighbor_files_not_allowed:"));
    permission.allowed = false;
    assert!(Capability::MemoryRead.require(&permission).is_err());
    assert!(Capability::Spawn.require(&permission).is_err());
}

#[test]
fn rpc_and_http_classify_memory_secrets_and_creation_routes_before_dispatch() {
    for method in ["mem.search", "mem.get", "mem.wake_up"] {
        assert!(matches!(
            rpc_capability(method),
            Some(Capability::MemoryRead)
        ));
    }
    for method in ["mem.save", "mem.append", "mem.delete"] {
        assert!(matches!(
            rpc_capability(method),
            Some(Capability::MemoryWrite)
        ));
    }
    assert!(matches!(
        rpc_capability("mem.secret.get"),
        Some(Capability::Secrets)
    ));
    for method in ["tab.create", "agent.spawn", "plan.task.dispatch"] {
        assert!(matches!(rpc_capability(method), Some(Capability::Spawn)));
    }
    for path in [
        "/api/mem/search",
        "/api/mem/items/own-id",
        "/api/mem/wake-up",
    ] {
        assert!(matches!(
            http_capability("GET", path),
            Some(Capability::MemoryRead)
        ));
        assert!(matches!(
            http_capability("POST", path),
            Some(Capability::MemoryWrite)
        ));
    }
    assert!(matches!(
        http_capability("GET", "/api/secrets/item"),
        Some(Capability::Secrets)
    ));
    for path in [
        "/api/tabs",
        "/api/workspaces/own/tabs",
        "/api/agents/spawn",
        "/api/plans/p/tasks/t/respawn",
    ] {
        assert!(matches!(
            http_capability("POST", path),
            Some(Capability::Spawn)
        ));
    }
    assert!(http_capability("GET", "/api/tabs").is_none());
    assert!(rpc_capability("system.identify").is_none());
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
