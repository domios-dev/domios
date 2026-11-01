use super::*;
use async_trait::async_trait;
use codeterm_neighbors::{
    receive::InboxListener,
    registry::ReadSignal,
    wire::{Cursor, DeliveryResult, PeerKeys, SignedEnvelope},
};
use std::sync::atomic::{AtomicUsize, Ordering};

struct StatusPort {
    calls: AtomicUsize,
}
#[async_trait]
impl NeighborTransport for StatusPort {
    async fn send(&self, _: &SignedEnvelope) -> Result<DeliveryStatus> {
        panic!("not a send")
    }
    async fn listen_from_cursor(
        &self,
        _: &str,
        _: Option<&Cursor>,
    ) -> Result<Box<dyn InboxListener>> {
        panic!("not a listener")
    }
    async fn ack(&self, _: &str, _: &str) -> Result<DeliveryResult> {
        panic!("not an ACK")
    }
    async fn peer_keys(&self, _: &str) -> Result<PeerKeys> {
        panic!("not a key lookup")
    }
    async fn delivery_status(&self, _: &str) -> Result<DeliveryResult> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(DeliveryResult {
            status: DeliveryStatus::Delivered,
            reason: None,
            delivered_at: Some(2000),
            read_at: Some(1000),
        })
    }
}

#[tokio::test]
async fn read_before_delivery_survives_and_a_hidden_read_requests_bounded_catchup() {
    use crate::service::neighbors::history::Entry;
    for ordering in 0..3 {
        let read_signal_seen = ordering != 1;
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
        let message = "b".repeat(32);
        let generation = fixture.service.neighbor_history_generation();
        let entry:Entry = serde_json::from_value(json!({"direction":"out","author":"me","sender_uid":"owner","body":"opaque","at":1,"message_id":message,"kind":"question","submission":"accepted","delivery_status":if ordering==2{"not_delivered"}else{"pending"}})).unwrap();
        fixture
            .service
            .append_neighbor_history(&metadata, &friendship, entry.clone(), generation)
            .await
            .unwrap();
        let anchor = if ordering == 2 {
            let mut later = entry;
            later.message_id = "c".repeat(32);
            later.delivery_status = Some(DeliveryStatus::Pending);
            let anchor = later.message_id.clone();
            fixture
                .service
                .append_neighbor_history(&metadata, &friendship, later, generation)
                .await
                .unwrap();
            anchor
        } else {
            message.clone()
        };
        if read_signal_seen {
            fixture
                .service
                .apply_neighbor_read_signal(
                    &metadata,
                    "owner",
                    &ReadSignal {
                        friendship_id: friendship.clone(),
                        message_id: anchor,
                        read_at: 1000,
                    },
                    generation,
                )
                .await
                .unwrap();
        }
        let delivery = DeliverySignal {
            reason: None,
            friendship_id: friendship.clone(),
            message_id: message.clone(),
            status: DeliveryStatus::Delivered,
            at: 2000,
        };
        fixture
            .service
            .apply_neighbor_delivery_signal(&metadata, "owner", &delivery, generation)
            .await
            .unwrap();
        let port = StatusPort {
            calls: AtomicUsize::new(0),
        };
        let catch_up = fixture
            .service
            .catch_up_neighbor_delivered_read(&metadata, &port, "owner", &delivery, generation)
            .await
            .unwrap();
        assert_eq!(catch_up, !read_signal_seen);
        assert_eq!(
            port.calls.load(Ordering::SeqCst),
            usize::from(!read_signal_seen)
        );
        let saved = metadata.history(&friendship).await.unwrap();
        assert_eq!(saved.entries[0].read_at, Some(1000));
        assert_eq!(
            saved.entries[0].delivery_status,
            Some(DeliveryStatus::Delivered)
        );
        assert!(!fixture
            .service
            .catch_up_neighbor_delivered_read(&metadata, &port, "owner", &delivery, generation)
            .await
            .unwrap());
    }
}
