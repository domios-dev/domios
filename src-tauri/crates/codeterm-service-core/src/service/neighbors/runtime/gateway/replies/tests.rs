use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};
use codeterm_neighbors::wire::{InboxDocument, Kind, Timestamp};
use serde_json::json;

fn message(kind: Kind, reply: Option<String>, id: usize) -> InboxMessage {
    let golden: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../codeterm-neighbors/test/golden-v2.json"
    )))
    .unwrap();
    let mut envelope: codeterm_neighbors::wire::Envelope =
        serde_json::from_value(golden["envelope"].clone()).unwrap();
    envelope.kind = kind;
    envelope.friendship_id = "a".repeat(64);
    envelope.message_id = format!("{id:032x}");
    envelope.in_reply_to = reply;
    InboxMessage {
        id: format!("{}_{}", envelope.sender_uid, envelope.message_id),
        doc: InboxDocument {
            envelope,
            signature: "opaque-signature".into(),
            received_at: Timestamp {
                seconds: 1,
                nanoseconds: 0,
            },
            expires_at: Timestamp {
                seconds: 2,
                nanoseconds: 0,
            },
        },
    }
}

fn gateway(fixture: &ServiceFixture) -> Gateway {
    *fixture.service.neighbor_cell().receiver.lock() =
        Some(codeterm_neighbors::adapter::ReceiverStatus {
            receiver_revision: 1,
            receiver_installation_id: Some("machine".into()),
            receiver_key_id: None,
            receiver_machine_label: Some("Here".into()),
            receiving_enabled: true,
            is_me: true,
            legacy_active: false,
        });
    let (_, stopped) = tokio::sync::watch::channel(false);
    Gateway {
        service: fixture.service.clone(),
        metadata: Metadata::new(fixture.temp.path().to_owned(), "owner"),
        friendship: "a".repeat(64),
        tab: String::new(),
        reply_target: None,
        generation: 0,
        history_generation: fixture.service.neighbor_history_generation(),
        stopped,
    }
}

async fn outgoing(gateway: &Gateway, friendship: &str, id: usize, author: serde_json::Value) {
    let entry = serde_json::from_value(json!({"direction":"out","author":author,"sender_uid":"owner","body":"opaque original question","at":1,"message_id":format!("{id:032x}"),"kind":"question"})).unwrap();
    gateway
        .service
        .append_neighbor_history(
            &gateway.metadata,
            friendship,
            entry,
            gateway.history_generation,
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn owner_and_unknown_replies_are_conversation_only_without_any_concierge() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let gateway = gateway(&fixture);
    outgoing(&gateway, &gateway.friendship, 1, json!("me")).await;
    for (id, kind, reply) in [
        (2, Kind::Answer, Some(format!("{:032x}", 1))),
        (3, Kind::Refusal, Some("c".repeat(32))),
        (4, Kind::Answer, None),
    ] {
        let incoming = message(kind, reply, id);
        assert!(matches!(
            gateway
                .deliver(&incoming, Zeroizing::new("opaque reply".into()))
                .await
                .unwrap(),
            GatewayReceipt::Delivered
        ));
        let page = gateway
            .metadata
            .history_page(&gateway.friendship, None, 50)
            .await
            .unwrap();
        assert_eq!(page["entries"][0]["handling_state"], "no_reply");
        assert_eq!(
            page["entries"][0]["message_id"],
            incoming.doc.envelope.message_id
        );
        assert!(gateway
            .metadata
            .incoming_received_by(&gateway.friendship, &incoming.doc.envelope.message_id)
            .await
            .unwrap()
            .is_none());
    }
    assert!(gateway
        .metadata
        .read::<Receipt>(format!("gateway-{}.json", gateway.friendship))
        .await
        .unwrap()
        .is_none());
    assert!(gateway
        .metadata
        .read::<Receipt>(format!("gateway-replies-{}.json", gateway.friendship))
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn linked_agent_replies_select_the_exact_author_and_never_another_friend_or_closed_tab() {
    let (mut mux, general) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let worker = mux.create_tab(&workspace).unwrap();
    for id in [&general, &worker] {
        let tab = mux.get_tab_mut(id).unwrap();
        tab.agent_type = Some("fixture".into());
        tab.agent_session_id = Some(format!("session-{id}"));
        tab.session_binding_nonce = Some(format!("nonce-{id}"));
        tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    }
    mux.get_tab_mut(&worker).unwrap().role = Some(crate::multiplexer::types::AgentRole::Worker);
    let fixture = ServiceFixture::new(mux);
    let gateway = gateway(&fixture);
    for (id, agent) in [(1, &general), (2, &worker)] {
        outgoing(
            &gateway,
            &gateway.friendship,
            id,
            json!({"agent":agent,"name":"opaque"}),
        )
        .await;
        let incoming = message(Kind::Answer, Some(format!("{id:032x}")), id + 10);
        assert_eq!(
            gateway
                .reply_target_for(&incoming)
                .await
                .unwrap()
                .unwrap()
                .tab_id,
            *agent
        );
    }
    outgoing(
        &gateway,
        &"b".repeat(64),
        3,
        json!({"agent":general,"name":"opaque"}),
    )
    .await;
    assert!(gateway
        .reply_target_for(&message(Kind::Refusal, Some(format!("{:032x}", 3)), 14))
        .await
        .unwrap()
        .is_none());
    fixture.service.close_tab_async(&worker).await.unwrap();
    assert!(gateway
        .reply_target_for(&message(Kind::Answer, Some(format!("{:032x}", 2)), 15))
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn only_confirmed_exact_session_delivery_stamps_a_private_receipt() {
    let (mut mux, general) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&general).unwrap();
    tab.agent_type = Some("fixture".into());
    tab.agent_session_id = Some("opaque-session".into());
    tab.session_binding_nonce = Some("opaque-nonce".into());
    tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    let fixture = ServiceFixture::new(mux);
    let mut gateway = gateway(&fixture);
    let target = gateway.delivery_target(&general).unwrap();
    gateway.tab = general.clone();
    gateway.reply_target = Some(target.clone());
    let first = message(Kind::Answer, None, 10);
    let second = message(Kind::Refusal, None, 11);
    for incoming in [&first, &second] {
        gateway
            .service
            .append_neighbor_history(
                &gateway.metadata,
                &gateway.friendship,
                crate::service::neighbors::handling::incoming(incoming, "opaque", None),
                gateway.history_generation,
            )
            .await
            .unwrap();
    }
    gateway
        .save_receipt(
            &first,
            "opaque-reply-receipt.json",
            ReceiptState::SubmitUnconfirmed,
        )
        .await
        .unwrap();
    assert!(gateway
        .metadata
        .incoming_received_by(&gateway.friendship, &first.doc.envelope.message_id)
        .await
        .unwrap()
        .is_none());
    gateway
        .confirm_delivery(&first, Some(target.clone()))
        .await
        .unwrap();
    let received = gateway
        .metadata
        .incoming_received_by(&gateway.friendship, &first.doc.envelope.message_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(received.tab_id, general);
    assert_eq!(received.binding_nonce, "opaque-nonce");
    let page = gateway
        .metadata
        .history_page(&gateway.friendship, None, 50)
        .await
        .unwrap();
    assert!(!page.to_string().contains("opaque-nonce"));
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&general)
        .unwrap()
        .session_binding_nonce = Some("new-nonce".into());
    gateway
        .confirm_delivery(&second, Some(target))
        .await
        .unwrap();
    assert!(gateway
        .metadata
        .incoming_received_by(&gateway.friendship, &second.doc.envelope.message_id)
        .await
        .unwrap()
        .is_none());
}
