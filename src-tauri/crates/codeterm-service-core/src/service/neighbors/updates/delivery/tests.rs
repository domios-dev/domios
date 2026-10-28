use super::*;

#[tokio::test]
async fn receiver_move_settlement_is_persisted_for_pending_outgoing_history() {
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friendship = "a".repeat(64);
    let outgoing = entry(1, "owner", "out", DeliveryStatus::Pending);
    let generation = fixture.service.neighbor_history_generation();
    fixture
        .service
        .append_neighbor_history(&metadata, &friendship, outgoing.clone(), generation)
        .await
        .unwrap();
    let signal = DeliverySignal {
        friendship_id: friendship.clone(),
        message_id: outgoing.message_id,
        status: DeliveryStatus::NotDelivered,
        reason: Some(codeterm_neighbors::wire::DeliveryReason::ReceiverMoved),
        at: 100,
    };
    fixture
        .service
        .apply_neighbor_delivery_signal(&metadata, "owner", &signal, generation)
        .await
        .unwrap();
    let saved = metadata.history(&friendship).await.unwrap();
    assert_eq!(
        saved.entries[0].delivery_status,
        Some(DeliveryStatus::NotDelivered)
    );
    assert_eq!(saved.entries[0].reason.as_deref(), Some("receiver_moved"));
    assert!(fixture
        .ui
        .0
        .lock()
        .iter()
        .any(|(event, body)| event == "neighbor-history-changed"
            && body["friendship_id"] == friendship));
}
use crate::service::neighbors::{history::Entry, storage::Settings};

fn entry(id: usize, uid: &str, direction: &str, status: DeliveryStatus) -> Entry {
    serde_json::from_value(json!({
        "direction":direction,"author":"me","sender_uid":uid,"body":"opaque",
        "at":id,"message_id":format!("{id:032x}"),"kind":"question",
        "delivery_status":status
    }))
    .unwrap()
}

#[test]
fn signal_revisions_catch_up_once_per_connection_or_gap_and_ignore_replays() {
    let mut revisions = Revisions::default();
    assert_eq!(revisions.observe(Some(7)), Some(true));
    assert_eq!(revisions.observe(Some(8)), Some(false));
    assert_eq!(revisions.observe(Some(8)), None);
    assert_eq!(revisions.observe(Some(6)), None);
    assert_eq!(revisions.observe(Some(11)), Some(true));
    assert_eq!(revisions.observe(Some(12)), Some(false));
    let mut reconnect = Revisions::default();
    assert_eq!(reconnect.observe(Some(12)), Some(true));
    assert_eq!(reconnect.observe(Some(12)), None);
    let mut empty = Revisions::default();
    assert_eq!(empty.observe(None), Some(true));
    assert_eq!(empty.observe(None), Some(false));
}

#[tokio::test]
async fn catchup_selects_bounded_current_account_outgoing_entries_until_read() {
    let temp = tempfile::tempdir().unwrap();
    let metadata = Metadata::new(temp.path().to_owned(), "owner");
    let foreign = Metadata::new(temp.path().to_owned(), "foreign-account");
    let friendship = "a".repeat(64);
    let another_friendship = "b".repeat(64);
    let mut settings = Settings::default();
    settings
        .permissions
        .insert(friendship.clone(), Default::default());
    settings
        .permissions
        .insert(another_friendship.clone(), Default::default());
    metadata.save_settings(settings).await.unwrap();
    let mut entries: Vec<_> = (1..=MAX_PENDING + 10)
        .map(|id| entry(id, "owner", "out", DeliveryStatus::Pending))
        .collect();
    entries.push(entry(900, "owner", "in", DeliveryStatus::Pending));
    entries.push(entry(901, "other", "out", DeliveryStatus::Pending));
    entries.push(entry(902, "owner", "out", DeliveryStatus::Delivered));
    let mut read = entry(903, "owner", "out", DeliveryStatus::Delivered);
    read.read_at = Some(1000);
    entries.push(read);
    metadata
        .write(
            format!("history-{friendship}.json"),
            json!({"ended":false,"entries":entries}),
        )
        .await
        .unwrap();
    let more: Vec<_> = (1000..1200)
        .map(|id| entry(id, "owner", "out", DeliveryStatus::Pending))
        .collect();
    metadata
        .write(
            format!("history-{another_friendship}.json"),
            json!({"ended":false,"entries":more}),
        )
        .await
        .unwrap();
    let pending = metadata.pending_outgoing("owner").await.unwrap();
    assert_eq!(pending.len(), MAX_PENDING);
    assert!(pending.contains(&format!("{:032x}", 902)));
    for id in [900, 901, 903] {
        assert!(!pending.contains(&format!("{id:032x}")));
    }
    assert!(foreign.pending_outgoing("owner").await.unwrap().is_empty());
}

#[tokio::test]
async fn delivery_signal_updates_only_own_outgoing_history_and_emits_change_without_polling() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let service = &fixture.service;
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friendship = "a".repeat(64);
    let generation = service.neighbor_history_generation();
    let outgoing = entry(1, "owner", "out", DeliveryStatus::Pending);
    service
        .append_neighbor_history(&metadata, &friendship, outgoing.clone(), generation)
        .await
        .unwrap();
    let mut signal = DeliverySignal {
        reason: None,
        at: 100,
        friendship_id: "b".repeat(64),
        message_id: outgoing.message_id.clone(),
        status: DeliveryStatus::Delivered,
    };
    service
        .apply_neighbor_delivery_signal(&metadata, "owner", &signal, generation)
        .await
        .unwrap();
    assert_eq!(
        metadata.history(&friendship).await.unwrap().entries[0].delivery_status,
        Some(DeliveryStatus::Pending)
    );
    signal.friendship_id = friendship.clone();
    service
        .apply_neighbor_delivery_signal(&metadata, "foreign", &signal, generation)
        .await
        .unwrap();
    assert_eq!(
        metadata.history(&friendship).await.unwrap().entries[0].delivery_status,
        Some(DeliveryStatus::Pending)
    );
    fixture.ui.0.lock().clear();
    service
        .apply_neighbor_delivery_signal(&metadata, "owner", &signal, generation)
        .await
        .unwrap();
    let history = metadata.history(&friendship).await.unwrap();
    assert_eq!(
        history.entries[0].delivery_status,
        Some(DeliveryStatus::Delivered)
    );
    assert_eq!(history.entries[0].submission, Some(Submission::Accepted));
    assert!(fixture
        .ui
        .0
        .lock()
        .iter()
        .any(|(name, value)| name == "neighbor-history-changed"
            && value["friendship_id"] == friendship));
    fixture.ui.0.lock().clear();
    service
        .apply_neighbor_delivery_signal(&metadata, "owner", &signal, generation)
        .await
        .unwrap();
    assert!(fixture.ui.0.lock().is_empty());
    service
        .update_neighbor_history_status(
            &metadata,
            &outgoing.message_id,
            HistoryReceipt {
                delivery: DeliveryStatus::Pending.into(),
                rejection: None,
                submission: Some(Submission::Accepted),
            },
            generation,
        )
        .await
        .unwrap();
    assert_eq!(
        metadata.history(&friendship).await.unwrap().entries[0].delivery_status,
        Some(DeliveryStatus::Delivered)
    );
    let expired = entry(2, "owner", "out", DeliveryStatus::Pending);
    service
        .append_neighbor_history(&metadata, &friendship, expired.clone(), generation)
        .await
        .unwrap();
    signal.message_id = expired.message_id;
    signal.status = DeliveryStatus::NotDelivered;
    service
        .apply_neighbor_delivery_signal(&metadata, "owner", &signal, generation)
        .await
        .unwrap();
    assert_eq!(
        metadata.history(&friendship).await.unwrap().entries[1].delivery_status,
        Some(DeliveryStatus::NotDelivered)
    );
    service
        .update_neighbor_history_status(
            &metadata,
            &signal.message_id,
            HistoryReceipt {
                delivery: DeliveryStatus::Pending.into(),
                rejection: None,
                submission: Some(Submission::Accepted),
            },
            generation,
        )
        .await
        .unwrap();
    assert_eq!(
        metadata.history(&friendship).await.unwrap().entries[1].delivery_status,
        Some(DeliveryStatus::NotDelivered)
    );
    service
        .neighbor_cell()
        .generation
        .fetch_add(1, Ordering::AcqRel);
    assert_eq!(
        service
            .apply_neighbor_delivery_signal(&metadata, "owner", &signal, generation)
            .await,
        Err(Error::Revoked)
    );
    service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = false;
    assert_eq!(
        service
            .apply_neighbor_delivery_signal(&metadata, "owner", &signal, generation)
            .await,
        Err(Error::Disabled)
    );
}
