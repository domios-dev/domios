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
        receiver_key_id: None,
        receiver_machine_label: Some("Here".into()),
        receiving_enabled: true,
        is_me: true,
        legacy_active: false,
    }
}

#[tokio::test]
async fn missing_concierge_never_admits_acknowledges_or_advances_a_cursor() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    *service.neighbor_cell().receiver.lock() = Some(receiving());
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friendship = "a".repeat(64);
    let (_, stopped) = tokio::sync::watch::channel(false);
    let session = listener::Session {
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
            enrollment: None,
        }),
        scope: KeyScope::installation("machine", "owner").unwrap(),
        metadata: metadata.clone(),
        friendship: friendship.clone(),
        tab: None,
        generation: 0,
        receiver_revision: 1,
        retry: Arc::new(tokio::sync::Notify::new()),
        stopped,
    };
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
    assert_eq!(view["state"], "concierge_missing");
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
        },
    );
    let local = Metadata::new(fixture.temp.path().to_owned(), "owner");
    local.save_settings(settings.clone()).await.unwrap();
    let mut receiver = receiving();
    assert_eq!(
        service.neighbor_concierge_view(&friendship, &settings, &receiver)["state"],
        "ready"
    );
    receiver.is_me = false;
    receiver.receiver_machine_label = Some("Other machine".into());
    *service.neighbor_cell().receiver.lock() = Some(receiver.clone());
    service.stop_neighbor_listeners();
    let view = service.neighbor_concierge_view(&friendship, &settings, &receiver);
    assert_eq!(view["state"], "receiving_elsewhere");
    assert_eq!(view["receiver_machine_label"], "Other machine");
    assert_eq!(view["concierge_missing"], false);
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
