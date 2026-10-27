use super::*;
use codeterm_neighbors::wire::{InboxDocument, Timestamp};

#[tokio::test]
async fn disabled_gateway_never_admits_or_records_a_message() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
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
    fixture
        .service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = false;
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let (_, stopped) = tokio::sync::watch::channel(false);
    let gateway = Gateway {
        service: fixture.service.clone(),
        metadata: metadata.clone(),
        friendship: "f".into(),
        tab: "missing".into(),
        generation: 0,
        stopped,
    };
    let golden: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../codeterm-neighbors/test/golden-v2.json"
    )))
    .unwrap();
    let message = InboxMessage {
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
    assert!(gateway.current().is_ok());
}
