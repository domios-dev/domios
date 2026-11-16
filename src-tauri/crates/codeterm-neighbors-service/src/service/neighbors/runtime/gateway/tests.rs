use super::*;
use crate::prelude::*;
use codeterm_neighbors::wire::{InboxDocument, Timestamp};

#[test]
fn gateway_rejects_a_different_friendship_or_a_descendant_before_delivery() {
    let (mut mux, general) = crate::multiplexer::state::MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let tab = mux.create_tab(&workspace).unwrap();
    mux.get_tab_mut(&tab).unwrap().neighbor_authority =
        Some(crate::multiplexer::types::NeighborAuthority {
            uid: "owner".into(),
            friendship_id: "a".repeat(64),
            root_tab_id: None,
        });
    let fixture = crate::service::test_fixture::crate::test_support::fixture(mux);
    fixture
        .service
        .set_neighbor_friend_receiver_for_test(&"a".repeat(64), Some(true), 1);
    fixture
        .service
        .set_neighbor_friend_receiver_for_test(&"b".repeat(64), Some(true), 1);

    let (_stop, stopped) = tokio::sync::watch::channel(false);
    let mut gateway = Gateway {
        reply_target: None,
        service: fixture.service.clone(),
        metadata: Metadata::new(fixture.temp.path().to_owned(), "owner"),
        friendship: "b".repeat(64),
        tab: tab.clone(),
        generation: 0,
        history_generation: 0,
        stopped,
    };
    assert_eq!(gateway.current(), Err(Error::Gateway));
    gateway.friendship = "a".repeat(64);
    assert!(gateway.current().is_ok());
    assert!(gateway.recipient_matches("owner"));
    assert!(!gateway.recipient_matches("other-owner"));
    let authority = fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab)
        .unwrap()
        .neighbor_authority
        .take();
    assert_eq!(gateway.current(), Err(Error::Gateway));
    assert!(!gateway.recipient_matches("owner"));
    {
        let mut mux = fixture.service.mux.write();
        let target = mux.get_tab_mut(&tab).unwrap();
        target.agent_type = Some("fixture".into());
        target.agent_session_id = Some("own-session".into());
        target.session_binding_nonce = Some("own-nonce".into());
    }
    gateway.reply_target = Some(Target {
        tab_id: tab.clone(),
        session_id: "own-session".into(),
        binding_nonce: "own-nonce".into(),
    });
    assert!(gateway.current().is_ok());
    assert!(gateway.recipient_matches("owner"));
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab)
        .unwrap()
        .session_binding_nonce = Some("foreign-nonce".into());
    assert_eq!(gateway.current(), Err(Error::Gateway));
    gateway.reply_target = None;
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab)
        .unwrap()
        .neighbor_authority = authority;
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab)
        .unwrap()
        .neighbor_authority
        .as_mut()
        .unwrap()
        .root_tab_id = Some("other-root".into());
    assert_eq!(gateway.current(), Err(Error::Gateway));
}

#[tokio::test]
async fn disabled_gateway_never_admits_or_records_a_message() {
    let (mut mux, general) = crate::multiplexer::state::MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let untagged = mux.create_tab(&workspace).unwrap();
    mux.get_tab_mut(&untagged).unwrap().agent_type = Some("fixture".into());
    let fixture = crate::service::test_fixture::crate::test_support::fixture(mux);
    fixture
        .service
        .set_neighbor_friend_receiver_for_test("f", Some(true), 1);

    fixture
        .service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = false;
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let (_, stopped) = tokio::sync::watch::channel(false);
    let mut gateway = Gateway {
        reply_target: None,
        service: fixture.service.clone(),
        metadata: metadata.clone(),
        friendship: "f".into(),
        tab: "missing".into(),
        generation: 0,
        history_generation: 0,
        stopped,
    };
    let golden: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../codeterm-neighbors/test/golden-v2.json"
    )))
    .unwrap();
    let mut message = InboxMessage {
        id: "m".into(),
        doc: InboxDocument {
            envelope: serde_json::from_value(golden["envelope"].clone()).unwrap(),
            signature: golden["signature"].as_str().unwrap().into(),
            received_at: Timestamp {
                seconds: 1,
                nanoseconds: 0,
            },
            expires_at: Timestamp {
                seconds: 2,
                nanoseconds: 0,
            },
        },
    };
    assert!(matches!(
        gateway
            .deliver(&message, Zeroizing::new("opaque".into()))
            .await,
        Err(Error::Disabled)
    ));
    assert!(metadata
        .read::<Receipt>("gateway-f.json".into())
        .await
        .unwrap()
        .is_none());
    fixture
        .service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = true;
    assert_eq!(gateway.current(), Err(Error::Gateway));
    gateway.tab = untagged;
    for kind in [
        codeterm_neighbors::wire::Kind::Question,
        codeterm_neighbors::wire::Kind::Answer,
    ] {
        message.doc.envelope.kind = kind;
        assert!(matches!(
            gateway
                .deliver(&message, Zeroizing::new("opaque".into()))
                .await,
            Err(Error::Gateway)
        ));
    }
    assert!(metadata
        .read::<Receipt>("gateway-f.json".into())
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn restored_bound_pty_concierge_wakes_and_becomes_ready_without_ui_activation() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let (mut mux, general) = crate::multiplexer::state::MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let tab = mux.create_tab(&workspace).unwrap();
    let concierge = mux.get_tab_mut(&tab).unwrap();
    concierge.agent_type = Some("pty-concierge-fixture".into());
    concierge.agent_session_id = Some("opaque-restored-session".into());
    concierge.session_binding_nonce = Some("opaque-restored-nonce".into());
    concierge.reported_session_binding_nonce = Some("opaque-restored-nonce".into());
    concierge.restored_from_snapshot = true;
    concierge.frozen = false;
    concierge.boot_stage = Some(codeterm_types::pty::BootStage::PtyStarting);
    let fixture = crate::service::test_fixture::crate::test_support::fixture(mux);
    let service = &fixture.service;
    service.set_neighbor_friend_receiver_for_test(&"a".repeat(64), Some(true), 1);
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friend = "a".repeat(64);
    let mut settings = crate::service::neighbors::storage::Settings::default();
    settings.bindings.insert(friend.clone(), tab.clone());
    settings.permissions.insert(
        friend.clone(),
        codeterm_neighbors::permissions::Permission::default(),
    );
    metadata.save_settings(settings).await.unwrap();
    service.tag_neighbor_root("owner", &friend, &tab).unwrap();
    let restored_settings = metadata.settings().await.unwrap();
    service.remember_neighbor_concierge_accounts("owner", &restored_settings);
    crate::service::lazy_restore::initialize(service, 0);
    assert!(service.mux.read().get_tab(&tab).unwrap().frozen);
    let calls = Arc::new(AtomicUsize::new(0));
    let invoked = calls.clone();
    let captured = service.clone();
    let exact = tab.clone();
    let _boundary = crate::service::lazy_restore::test_boundary::Boundary::install(
        &tab,
        Arc::new(move || {
            invoked.fetch_add(1, Ordering::SeqCst);
            let mut mux = captured.mux.write();
            let restored = mux.get_tab_mut(&exact).unwrap();
            assert!(!restored.frozen);
            assert_eq!(
                restored.agent_session_id.as_deref(),
                Some("opaque-restored-session")
            );
            restored.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);
            restored.agent_booting = false;
            Ok(())
        }),
    );
    assert!(!service.neighbor_gateway_ready(&tab));
    for _ in 0..2 {
        service.prepare_neighbor_gateway(&tab).await.unwrap();
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(service.neighbor_gateway_ready(&tab));
    let saved = metadata.settings().await.unwrap();
    assert_eq!(saved.bindings[&friend], tab);
    let mux = service.mux.read();
    let restored = mux.get_tab(&tab).unwrap();
    assert!(!restored.frozen);
    assert_eq!(
        restored.agent_session_id.as_deref(),
        Some("opaque-restored-session")
    );
    assert_eq!(
        restored.session_binding_nonce.as_deref(),
        Some("opaque-restored-nonce")
    );
}
