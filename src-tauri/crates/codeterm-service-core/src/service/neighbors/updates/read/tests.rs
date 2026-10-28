use super::*;
use codeterm_neighbors::wire::DeliveryStatus;

#[tokio::test]
async fn read_signal_persists_sender_timestamps_and_emits_one_history_change() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture
        .service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = true;
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friendship = "a".repeat(64);
    let entries: Vec<_> = (1..=3).map(|id| json!({"direction":"out","author":"me","sender_uid":"owner","body":"opaque","at":id,"message_id":format!("{id:032x}"),"kind":"question","submission":"accepted","delivery_status":"pending"})).collect();
    metadata
        .write(
            format!("history-{friendship}.json"),
            json!({"ended":false,"entries":entries}),
        )
        .await
        .unwrap();
    let signal = ReadSignal {
        friendship_id: friendship.clone(),
        message_id: format!("{:032x}", 2),
        read_at: 1000,
    };
    let generation = fixture.service.neighbor_history_generation();
    fixture
        .service
        .apply_neighbor_read_signal(&metadata, "owner", &signal, generation)
        .await
        .unwrap();
    let restored = metadata.history(&friendship).await.unwrap();
    assert_eq!(restored.entries[0].read_at, Some(1000));
    assert_eq!(restored.entries[1].delivered_at, Some(1000));
    assert_eq!(
        restored.entries[1].delivery_status,
        Some(DeliveryStatus::Delivered)
    );
    assert!(restored.entries[2].read_at.is_none());
    fixture
        .service
        .apply_neighbor_read_signal(&metadata, "owner", &signal, generation)
        .await
        .unwrap();
    assert_eq!(
        fixture
            .ui
            .0
            .lock()
            .iter()
            .filter(|(name, value)| name == "neighbor-history-changed"
                && value["friendship_id"] == friendship)
            .count(),
        1
    );
    let unknown = ReadSignal {
        message_id: "f".repeat(32),
        ..signal
    };
    assert!(
        !fixture
            .service
            .apply_neighbor_read_signal(&metadata, "owner", &unknown, generation)
            .await
            .unwrap(),
        "an anchor from another installation requires bounded status catch-up"
    );
    let page = metadata.history_page(&friendship, None, 50).await.unwrap();
    assert_eq!(page["entries"][1]["read_at"], 1000);
}

#[tokio::test]
async fn delivery_catchup_adds_read_time_to_an_already_delivered_entry() {
    use crate::service::neighbors::history::Entry;
    use codeterm_neighbors::wire::DeliveryResult;
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friendship = "b".repeat(64);
    let message = "c".repeat(32);
    let entry: Entry = serde_json::from_value(json!({"direction":"out","author":"me","sender_uid":"owner","body":"opaque","at":1,"message_id":message,"kind":"question","submission":"accepted","delivery_status":"delivered"})).unwrap();
    let generation = fixture.service.neighbor_history_generation();
    fixture
        .service
        .append_neighbor_history(&metadata, &friendship, entry, generation)
        .await
        .unwrap();
    let receipt = DeliveryResult {
        status: DeliveryStatus::Delivered,
        reason: None,
        delivered_at: Some(200),
        read_at: Some(300),
    };
    fixture
        .service
        .update_neighbor_history_status(&metadata, &message, receipt.into(), generation)
        .await
        .unwrap();
    fixture
        .service
        .update_neighbor_history_status(
            &metadata,
            &message,
            DeliveryStatus::Pending.into(),
            generation,
        )
        .await
        .unwrap();
    let history = metadata.history(&friendship).await.unwrap();
    assert_eq!(history.entries[0].read_at, Some(300));
    assert_eq!(history.entries[0].delivered_at, Some(200));
    assert_eq!(
        history.entries[0].delivery_status,
        Some(DeliveryStatus::Delivered)
    );
}
