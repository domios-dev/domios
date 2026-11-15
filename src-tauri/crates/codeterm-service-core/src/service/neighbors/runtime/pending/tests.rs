use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};
use async_trait::async_trait;

struct NoRemoteCalls;

#[async_trait]
impl codeterm_neighbors::adapter::AuthenticatedCall for NoRemoteCalls {
    async fn invoke(&self, _: &str, _: Value) -> codeterm_neighbors::Result<Value> {
        panic!("Missing concierge cannot admit or acknowledge a message")
    }
    async fn id_token(&self) -> codeterm_neighbors::Result<Zeroizing<String>> {
        panic!("No inbox admission is authorized")
    }
}

fn receiving() -> ReceiverStatus {
    ReceiverStatus {
        receiver_revision: 1,
        receiver_installation_id: Some("machine".into()),
        receiver_encryption_key_id: None,
        receiver_machine_label: Some("Here".into()),
        receiving_enabled: true,
        is_me: true,
    }
}

#[test]
fn restored_concierge_reports_starting_until_its_own_runtime_is_live_and_ready() {
    use crate::multiplexer::types::TabBacking;
    use codeterm_types::pty::BootStage;

    let (mut mux, general) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let gateway = mux.create_tab(&workspace).unwrap();
    let tab = mux.get_tab_mut(&gateway).unwrap();
    tab.agent_type = Some("neighbor-runtime-fixture".into());
    tab.agent_session_id = Some("saved-thread".into());
    tab.session_binding_nonce = Some("own-nonce".into());
    tab.reported_session_binding_nonce = Some("own-nonce".into());
    tab.restored_from_snapshot = true;
    tab.agent_booting = true;
    tab.boot_stage = Some(BootStage::AgentLaunching);
    tab.backing = TabBacking::ChatBackend {
        plugin_id: "neighbor-runtime-fixture".into(),
        session_id: "saved-thread".into(),
    };
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    let manifest = codeterm_plugin::js_runtime::PluginManifest::from_json_str(
        &json!({"id":"neighbor-runtime-fixture","version":"1.0.0","displayName":"Fixture",
            "capabilities":{"agentProvider":true,"chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},
                "agentRuntime":{"runtimeModes":[],"runtime":"stdio-jsonrpc",
                    "command":{"binary":"fixture-cli","args":[]},
                    "requestMethods":{"initialize":["handshake"]},"notificationMethods":[]}},
            "permissions":{"subprocess":{"allow":["fixture-cli"]}}})
        .to_string(),
    )
    .unwrap();
    let vm = Arc::new(
        codeterm_plugin::js_runtime::PluginVm::from_source(
            "neighbor-runtime-fixture",
            "module.exports={}".into(),
        )
        .unwrap(),
    );
    service.agent_registry.register_provider(Box::new(
        codeterm_plugin::js_runtime::JsProvider::new(manifest, vm),
    ));
    let friendship = "a".repeat(64);
    let mut settings = Settings::default();
    settings
        .bindings
        .insert(friendship.clone(), gateway.clone());
    settings
        .permissions
        .insert(friendship.clone(), Permission::default());
    let state =
        || service.neighbor_concierge_view(&friendship, &settings, &receiving())["state"].clone();
    assert_eq!(state(), "concierge_starting");
    {
        let mut mux = service.mux.write();
        let tab = mux.get_tab_mut(&gateway).unwrap();
        tab.agent_booting = false;
        tab.boot_stage = Some(BootStage::AgentReady);
    }
    assert_eq!(state(), "concierge_crashed");
    let prepared = service.mux.read().get_tab(&gateway).unwrap().clone();
    let _live = crate::service::agent_runtime::engine::test_support::live_runtime(&prepared);
    assert_eq!(state(), "ready");
    service
        .mux
        .write()
        .get_tab_mut(&gateway)
        .unwrap()
        .reported_session_binding_nonce = Some("foreign-nonce".into());
    assert_eq!(state(), "concierge_crashed");
}

#[test]
fn failed_concierge_projects_auth_provider_crash_and_closed_without_changing_pending() {
    use codeterm_types::pty::BootStage;

    let (mut mux, general) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let gateway = mux.create_tab(&workspace).unwrap();
    mux.get_tab_mut(&gateway).unwrap().agent_type = Some("fixture".into());
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    let friendship = "a".repeat(64);
    let mut settings = Settings::default();
    settings
        .bindings
        .insert(friendship.clone(), gateway.clone());
    settings
        .permissions
        .insert(friendship.clone(), Permission::default());
    service
        .neighbor_cell()
        .pending_counts
        .lock()
        .insert(friendship.clone(), 7);
    for (runtime, boot, expected) in [
        (
            json!({"state":"loginPending"}),
            BootStage::AgentLaunching,
            "concierge_auth_needed",
        ),
        (
            json!({"state":"loaded","auth":"signedOut"}),
            BootStage::AgentReady,
            "concierge_auth_needed",
        ),
        (
            json!({"state":"degraded","failureCode":"authentication_required"}),
            BootStage::SpawnFailed,
            "concierge_auth_needed",
        ),
        (
            json!({"state":"degraded","failureCode":"provider_refused"}),
            BootStage::SpawnFailed,
            "concierge_provider_refused",
        ),
        (
            json!({"state":"degraded","failureCode":"provider_refused","accessFailure":"neighbor_restore_grant_required"}),
            BootStage::SpawnFailed,
            "concierge_access_needed",
        ),
        (
            json!({"state":"unloaded","failureCode":"process_unavailable"}),
            BootStage::AgentReady,
            "concierge_crashed",
        ),
        (
            json!({"state":"degraded","failureCode":"protocol_error"}),
            BootStage::AgentReady,
            "concierge_crashed",
        ),
        (
            Value::Null,
            BootStage::SpawnFailed,
            "concierge_provider_refused",
        ),
        (
            Value::Null,
            BootStage::AgentReady,
            "concierge_provider_refused",
        ),
    ] {
        {
            let mut mux = service.mux.write();
            let tab = mux.get_tab_mut(&gateway).unwrap();
            tab.agent_runtime_state = (!runtime.is_null()).then_some(runtime);
            tab.boot_stage = Some(boot);
            tab.agent_booting = false;
        }
        let view = service.neighbor_concierge_view(&friendship, &settings, &receiving());
        assert_eq!(view["state"], expected);
        assert_eq!(view["gateway_tab"], gateway);
        assert_eq!(view["concierge_missing"], false);
        assert!(view["pending_count"].is_null());
        assert_eq!(settings.bindings[&friendship], gateway);
        assert_eq!(
            service.neighbor_cell().pending_counts.lock()[&friendship],
            7
        );
    }
    settings.permissions.remove(&friendship);
    assert_eq!(
        service.neighbor_concierge_view(&friendship, &settings, &receiving())["state"],
        "concierge_access_needed"
    );
    assert!(!settings.permissions.contains_key(&friendship));
    settings
        .permissions
        .insert(friendship.clone(), Permission::default());
    service
        .mux
        .write()
        .get_tab_mut(&gateway)
        .unwrap()
        .provider_signals
        .auth = Some(codeterm_provider_signals::AuthStatus {
        logged_in: false,
        auth_method: None,
        subscription_type: None,
        config_directory: None,
    });
    assert_eq!(
        service.neighbor_concierge_view(&friendship, &settings, &receiving())["state"],
        "concierge_auth_needed"
    );
    {
        let mut mux = service.mux.write();
        let tab = mux.get_tab_mut(&gateway).unwrap();
        tab.provider_signals.auth = None;
        tab.role = Some(codeterm_types::pty::AgentRole::Worker);
    }
    let invalid = service.neighbor_concierge_view(&friendship, &settings, &receiving());
    assert_eq!(invalid["state"], "concierge_invalid");
    assert!(invalid["gateway_tab"].is_null());
    service.mux.write().get_tab_mut(&gateway).unwrap().role = None;
    service.mux.write().close_tab(&gateway).unwrap();
    let closed = service.neighbor_concierge_view(&friendship, &settings, &receiving());
    assert_eq!(closed["state"], "concierge_closed");
    assert_eq!(closed["concierge_missing"], true);
    assert!(closed["gateway_tab"].is_null());
    assert_eq!(closed["pending_count"], 7);
    assert_eq!(settings.bindings[&friendship], gateway);
    assert!(service.mux.read().get_tab(&gateway).is_none());
    settings.bindings.clear();
    assert_eq!(
        service.neighbor_concierge_view(&friendship, &settings, &receiving())["state"],
        "concierge_missing"
    );
}

async fn receiving_session(
    fixture: &ServiceFixture,
    tab: Option<String>,
) -> (listener::Session, tokio::sync::watch::Sender<bool>) {
    let service = &fixture.service;
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friendship = "a".repeat(64);
    service.set_neighbor_friend_receiver_for_test(&friendship, Some(true), 1);
    let user = codeterm_account::Session {
        identity: codeterm_account::Identity {
            uid: "owner".into(),
            email: None,
            display_name: None,
            photo_url: None,
            provider: "fake".into(),
        },
        login_generation: 0,
        id_token: codeterm_account::Secret::new("fixture"),
        expires_at: None,
    };
    crate::service::neighbors::paid_fixture::bind(service, &user, fixture.temp.path()).await;
    let (shutdown, stopped) = tokio::sync::watch::channel(false);
    (
        listener::Session {
            service: service.clone(),
            transport: Arc::new(FirebaseTransport {
                calls: Arc::new(NoRemoteCalls),
                project: "fixture".into(),
                uid: "owner".into(),
                installation_id: "machine".into(),
            }),
            registry: Arc::new(FirebaseRegistry {
                calls: Arc::new(NoRemoteCalls),
                uid: "owner".into(),
                installation_id: "machine".into(),
                machine_label: "Here".into(),
            }),
            scope: KeyScope::installation("machine", "owner").unwrap(),
            metadata: metadata.clone(),
            friendship: friendship.clone(),
            tab,
            generation: 0,
            receiver_revision: 1,
            retry: Arc::new(tokio::sync::Notify::new()),
            stopped,
        },
        shutdown,
    )
}

#[tokio::test]
async fn lifecycle_changes_publish_without_inbox_activity_or_payload_replay() {
    let (mut mux, general) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let gateway = mux.create_tab(&workspace).unwrap();
    mux.get_tab_mut(&gateway).unwrap().agent_type = Some("fixture".into());
    let fixture = ServiceFixture::new(mux);
    let (session, _shutdown) = receiving_session(&fixture, Some(gateway.clone())).await;
    let service = &fixture.service;
    let mut settings = Settings::default();
    settings
        .bindings
        .insert(session.friendship.clone(), gateway.clone());
    settings
        .permissions
        .insert(session.friendship.clone(), Permission::default());
    session.metadata.save_settings(settings).await.unwrap();
    let (published, mut observed) = tokio::sync::mpsc::unbounded_channel();
    let listener = fixture
        .ui
        .listen("neighbor-concierge-status", move |value| {
            published.send(value.clone()).unwrap();
        });
    let watch = watch_lifecycle(&session);
    tokio::pin!(watch);
    let initial = tokio::select! {
        _ = &mut watch => panic!("lifecycle watch ended before its first projection"),
        value = tokio::time::timeout(std::time::Duration::from_secs(5), observed.recv()) => value.unwrap().unwrap(),
    };
    assert_eq!(initial["state"], "concierge_starting");
    for (state, code, expected) in [
        (
            "loginPending",
            "authentication_required",
            "concierge_auth_needed",
        ),
        ("degraded", "provider_refused", "concierge_provider_refused"),
        ("unloaded", "process_unavailable", "concierge_crashed"),
    ] {
        service
            .mux
            .write()
            .get_tab_mut(&gateway)
            .unwrap()
            .agent_runtime_state = Some(json!({"state":state,"failureCode":code}));
        service
            .events
            .publish_generic("agent-runtime-state", json!({"tab_id":gateway}));
        let view = tokio::select! {
            _ = &mut watch => panic!("lifecycle watch ended before publication"),
            value = tokio::time::timeout(std::time::Duration::from_secs(5), observed.recv()) => value.unwrap().unwrap(),
        };
        assert_eq!(view["state"], expected);
        assert_eq!(view["friendship_id"], session.friendship);
        assert_eq!(view["gateway_tab"], gateway);
    }
    service.mux.write().close_tab(&gateway).unwrap();
    service
        .events
        .publish_generic("mux-state-changed", json!({"generation": 1}));
    let closed = tokio::select! {
        _ = &mut watch => panic!("lifecycle watch ended before close publication"),
        value = tokio::time::timeout(std::time::Duration::from_secs(5), observed.recv()) => value.unwrap().unwrap(),
    };
    assert_eq!(closed["state"], "concierge_closed");
    let initial_closed = tokio::select! {
        _ = watch_lifecycle(&session) => panic!("lifecycle watch ended before its closed projection"),
        value = tokio::time::timeout(std::time::Duration::from_secs(5), observed.recv()) => value.unwrap().unwrap(),
    };
    assert_eq!(initial_closed["state"], "concierge_closed");
    assert!(session
        .metadata
        .load(&session.friendship)
        .await
        .unwrap()
        .is_none());
    assert!(session
        .metadata
        .history_page(&session.friendship, None, 1)
        .await
        .unwrap()["entries"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(service.mux.read().get_tab(&gateway).is_none());
    fixture.ui.unlisten(listener);
}

#[tokio::test]
async fn missing_concierge_never_admits_acknowledges_or_advances_a_cursor() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    let (session, _shutdown) = receiving_session(&fixture, None).await;
    let metadata = &session.metadata;
    let friendship = session.friendship.clone();
    let golden: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../codeterm-neighbors/test/golden-v2.json"
    )))
    .unwrap();
    let mut messages = [codeterm_neighbors::wire::InboxMessage {
        id: "opaque-message".into(),
        doc: codeterm_neighbors::wire::InboxDocument {
            envelope: serde_json::from_value(golden["envelope"].clone()).unwrap(),
            signature: golden["signature"].as_str().unwrap().into(),
            received_at: now(),
            expires_at: now(),
        },
    }];
    assert_eq!(session.receive(&mut messages).await, Err(Error::Gateway));
    assert!(metadata.load(&friendship).await.unwrap().is_none());
    assert!(
        metadata.history_page(&friendship, None, 1).await.unwrap()["entries"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let view = service.neighbor_concierge_view(&friendship, &Settings::default(), &receiving());
    assert_eq!(view["concierge_missing"], true);
    assert!(view["pending_count"].is_null());
    service
        .neighbor_cell()
        .pending_counts
        .lock()
        .insert(friendship.clone(), 3);
    publish(&session).await.unwrap();
    let events = fixture.ui.0.lock();
    let (_, view) = events
        .iter()
        .find(|(event, _)| event == "neighbor-concierge-status")
        .unwrap();
    assert_eq!(view["pending_count"], 3);
    assert_eq!(view["pending_count_capped"], false);
    assert_eq!(view["state"], "concierge_missing");
    drop(events);
    service
        .neighbor_cell()
        .pending_counts
        .lock()
        .insert(friendship.clone(), 100);
    assert_eq!(
        service.neighbor_concierge_view(&friendship, &Settings::default(), &receiving())
            ["pending_count_capped"],
        true
    );
}

#[tokio::test]
async fn receiver_move_keeps_local_concierge_and_never_copies_bindings_or_permissions() {
    let (mut mux, general) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let gateway = mux.create_tab(&workspace).unwrap();
    mux.get_tab_mut(&gateway).unwrap().agent_type = Some("fixture".into());
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    let friendship = "a".repeat(64);
    let mut settings = Settings::default();
    settings
        .bindings
        .insert(friendship.clone(), gateway.clone());
    settings.permissions.insert(
        friendship.clone(),
        Permission {
            allowed: true,
            scope: "opaque scope".into(),
            allow_tab_requests: false,
            allow_interactive_content: false,
            ..Default::default()
        },
    );
    let local = Metadata::new(fixture.temp.path().to_owned(), "owner");
    local.save_settings(settings.clone()).await.unwrap();
    let mut receiver = receiving();
    assert_eq!(
        service.neighbor_concierge_view(&friendship, &settings, &receiver)["state"],
        "concierge_starting"
    );
    receiver.is_me = false;
    receiver.receiver_machine_label = Some("Other machine".into());

    service.stop_neighbor_listeners();
    let view = service.neighbor_concierge_view(&friendship, &settings, &receiver);
    assert_eq!(view["state"], "receiving_elsewhere");
    assert_eq!(view["receiver_machine_label"], "Other machine");
    assert_eq!(view["concierge_missing"], false);
    assert!(view.get("not_receiving_reason").is_none());
    let lapsed = std::collections::BTreeSet::from(["machine".to_owned()]);
    let expired = serde_json::to_value(service.neighbor_concierge_projection(
        &friendship,
        &settings,
        &receiver,
        &lapsed,
    ))
    .unwrap();
    assert_eq!(
        expired["not_receiving_reason"],
        ServiceContext::receiver_view(&receiver, &lapsed)["not_receiving_reason"]
    );
    assert_eq!(expired["state"], "not_receiving");
    assert_eq!(expired["not_receiving_reason"], "membership_expired");
    assert!(service.mux.read().get_tab(&gateway).is_some());
    let another = Metadata::new(fixture.temp.path().join("other-installation"), "owner");
    let other_settings = another.settings().await.unwrap();
    assert!(other_settings.bindings.is_empty());
    assert!(other_settings.permissions.is_empty());
    let view = service.neighbor_concierge_view(&friendship, &other_settings, &receiving());
    assert_eq!(view["state"], "concierge_missing");
    assert_eq!(
        local.settings().await.unwrap().permissions[&friendship].scope,
        "opaque scope"
    );
}
