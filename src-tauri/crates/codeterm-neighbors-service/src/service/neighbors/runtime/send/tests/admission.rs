use super::*;
use std::sync::{atomic::Ordering, Arc};

async fn attempt(
    fixture: &ServiceFixture,
    metadata: &Metadata,
    store: &KeyFence,
    friendship: &str,
    message_id: &str,
) -> Error {
    let calls = Arc::new(NoNetwork);
    let transport = FirebaseTransport {
        calls: calls.clone(),
        project: "fixture".into(),
        uid: "owner".into(),
        installation_id: "machine".into(),
    };
    let registry = FirebaseRegistry {
        calls,
        uid: "owner".into(),
        installation_id: "machine".into(),
        machine_label: "Fixture".into(),
    };
    let lifecycle = InstallationKeys {
        store,
        registry: &registry,
        scope: KeyScope::installation("machine", "owner").unwrap(),
        uid: "owner",
    };
    fixture
        .service
        .send_neighbor_once(
            &transport,
            &lifecycle,
            metadata,
            Outgoing {
                message_id: Some(message_id.into()),
                authority: SendAuthority::Unrestricted,
                friendship: friendship.into(),
                kind: Kind::Question,
                body: OutboundBody::Granted(Zeroizing::new("opaque admitted send".into())),
                in_reply_to: None,
                author: crate::service::neighbors::history::Author::Person("me".into()),
            },
        )
        .await
        .err()
        .unwrap()
}

fn outgoing(id: u64, body: &str) -> crate::service::neighbors::history::Entry {
    serde_json::from_value(json!({"direction":"out","author":"me","sender_uid":"owner",
        "body":body,"at":id,"message_id":format!("{id:032x}"),"kind":"question"}))
    .unwrap()
}

fn fixture() -> ServiceFixture {
    let fixture = crate::test_support::fixture(MultiplexerState::new().0);
    fixture
        .service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = true;
    fixture
}

#[tokio::test]
async fn cross_friend_id_collision_is_refused_before_keys_or_network_without_repointing_delivery() {
    let fixture = fixture();
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let store = KeyFence::default();
    let first_friend = "a".repeat(64);
    let second_friend = "b".repeat(64);
    let id = format!("{:032x}", 1);
    assert_eq!(
        attempt(&fixture, &metadata, &store, &first_friend, &id).await,
        Error::SecretStore
    );
    fixture
        .service
        .append_neighbor_history(
            &metadata,
            &first_friend,
            outgoing(1, "opaque original"),
            fixture.service.neighbor_history_generation(),
        )
        .await
        .unwrap();
    assert_eq!(
        attempt(&fixture, &metadata, &store, &second_friend, &id).await,
        Error::OperationIdConflict
    );
    assert_eq!(store.0.load(Ordering::SeqCst), 1);
    assert_eq!(
        metadata.outgoing_friendship(&id).await.unwrap(),
        Some(first_friend)
    );
    assert!(metadata
        .history(&second_friend)
        .await
        .unwrap()
        .entries
        .is_empty());
}

#[tokio::test]
async fn admission_survives_count_and_byte_body_eviction_and_metadata_reopen() {
    for (count, body) in [(256, "opaque later".into()), (2, "x".repeat(400_000))] {
        let fixture = fixture();
        let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
        let store = KeyFence::default();
        let friend = "a".repeat(64);
        let id = format!("{:032x}", 1);
        assert_eq!(
            attempt(&fixture, &metadata, &store, &friend, &id).await,
            Error::SecretStore
        );
        fixture
            .service
            .append_neighbor_history(
                &metadata,
                &friend,
                outgoing(1, "opaque original"),
                fixture.service.neighbor_history_generation(),
            )
            .await
            .unwrap();
        let mut history = metadata.history(&friend).await.unwrap();
        for next in 2..count + 2 {
            history.entries.push(outgoing(next, &body));
        }
        history.bound().unwrap();
        assert!(!history.entries.iter().any(|entry| entry.message_id == id));
        metadata
            .write(format!("history-{friend}.json"), history)
            .await
            .unwrap();
        fixture
            .service
            .append_neighbor_history(
                &metadata,
                &friend,
                outgoing(258, "opaque last"),
                fixture.service.neighbor_history_generation(),
            )
            .await
            .unwrap();
        assert_eq!(metadata.outgoing_friendship(&id).await.unwrap(), None);
        let reopened = Metadata::new(fixture.temp.path().to_owned(), "owner");
        assert_eq!(
            attempt(&fixture, &reopened, &store, &friend, &id).await,
            Error::OperationIdConflict
        );
        assert_eq!(store.0.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn concurrent_account_admission_reserves_a_supplied_id_once() {
    let fixture = fixture();
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let store = KeyFence::default();
    let id = "e".repeat(32);
    let first = "a".repeat(64);
    let second = "b".repeat(64);
    let (one, two) = tokio::join!(
        attempt(&fixture, &metadata, &store, &first, &id),
        attempt(&fixture, &metadata, &store, &second, &id)
    );
    assert!([one, two].contains(&Error::OperationIdConflict));
    assert!([one, two].contains(&Error::SecretStore));
    assert_eq!(store.0.load(Ordering::SeqCst), 1);
}
