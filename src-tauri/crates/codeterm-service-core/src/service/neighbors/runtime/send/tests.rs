use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};

#[tokio::test]
async fn refusal_preflight_accepts_cyrillic_and_rejects_oversize_without_writing() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friendship = "a".repeat(64);
    let target = "b".repeat(32);
    let incoming = serde_json::from_value(json!({"direction":"in","author":"friend","sender_uid":"peer","body":"opaque incoming","at":1,"message_id":target,"kind":"question"})).unwrap();
    fixture
        .service
        .append_neighbor_history(
            &metadata,
            &friendship,
            incoming,
            fixture.service.neighbor_history_generation(),
        )
        .await
        .unwrap();
    let before = metadata.history_page(&friendship, None, 50).await.unwrap();
    let permission = Permission::default();
    let cyrillic = "\u{044f}".repeat(300);
    assert_eq!(cyrillic.len(), 600);
    let (accepted, paths) = prepare_outgoing_body(
        &metadata,
        &friendship,
        &permission,
        Kind::Refusal,
        OutboundBody::Granted(Zeroizing::new(cyrillic.clone())),
        Some(&target),
    )
    .await
    .unwrap();
    assert_eq!(accepted.as_str(), cyrillic);
    assert!(paths.is_empty());
    for size in [1024, 1025] {
        let result = prepare_outgoing_body(
            &metadata,
            &friendship,
            &permission,
            Kind::Refusal,
            OutboundBody::Granted(Zeroizing::new("x".repeat(size))),
            Some(&target),
        )
        .await;
        if size == 1024 {
            assert!(result.is_ok());
        } else {
            assert_eq!(
                result.unwrap_err(),
                Error::RefusalTooLong {
                    limit: 1024,
                    actual: 1025
                }
            );
        }
    }
    for target in [None, Some("cccccccccccccccccccccccccccccccc")] {
        assert_eq!(
            prepare_outgoing_body(
                &metadata,
                &friendship,
                &permission,
                Kind::Refusal,
                OutboundBody::Granted(Zeroizing::new("opaque refusal".into())),
                target
            )
            .await
            .unwrap_err(),
            Error::ReplyTargetUnknown
        );
    }
    assert_eq!(
        metadata.history_page(&friendship, None, 50).await.unwrap(),
        before
    );
}

struct NoNetwork;
#[async_trait::async_trait]
impl codeterm_neighbors::adapter::AuthenticatedCall for NoNetwork {
    async fn invoke(&self, _: &str, _: Value) -> codeterm_neighbors::Result<Value> {
        panic!("pre-send fence must not contact relay")
    }
}

#[derive(Default)]
struct KeyFence(std::sync::atomic::AtomicUsize);
#[async_trait::async_trait]
impl codeterm_neighbors::keys::KeyStore for KeyFence {
    async fn load(
        &self,
        _: &KeyScope,
        _: codeterm_neighbors::keys::Slot,
    ) -> codeterm_neighbors::Result<Option<Zeroizing<Vec<u8>>>> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(Error::SecretStore)
    }
    async fn save(
        &self,
        _: &KeyScope,
        _: codeterm_neighbors::keys::Slot,
        _: Zeroizing<Vec<u8>>,
    ) -> codeterm_neighbors::Result<()> {
        panic!("pre-send fence must not write keys")
    }
    async fn delete(
        &self,
        _: &KeyScope,
        _: codeterm_neighbors::keys::Slot,
    ) -> codeterm_neighbors::Result<()> {
        panic!("pre-send fence must not delete keys")
    }
}

#[tokio::test]
async fn stopped_requests_reject_later_answers_and_refusals_before_keys_network_or_history() {
    use crate::service::neighbors::history::{handling::State, Author};
    use std::sync::{atomic::Ordering, Arc};
    let fixture = ServiceFixture::new(MultiplexerState::new().0);
    let service = &fixture.service;
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friend = "a".repeat(64);
    let target = "b".repeat(32);
    let mut settings = crate::service::neighbors::storage::Settings::default();
    settings.permissions.insert(
        friend.clone(),
        Permission {
            allowed: true,
            scope: "opaque grant".into(),
            ..Default::default()
        },
    );
    metadata.save_settings(settings).await.unwrap();
    let incoming = serde_json::from_value(json!({"direction":"in","author":"friend","sender_uid":"peer","body":"opaque request","at":1,"message_id":target,"kind":"question","handling_state":"working"})).unwrap();
    service
        .append_neighbor_history(
            &metadata,
            &friend,
            incoming,
            service.neighbor_history_generation(),
        )
        .await
        .unwrap();
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
        enrollment: None,
    };
    let store = KeyFence::default();
    let lifecycle = InstallationKeys {
        store: &store,
        registry: &registry,
        scope: KeyScope::installation("machine", "owner").unwrap(),
        uid: "owner",
    };
    for stopped in [false, true] {
        if stopped {
            service
                .mutate_neighbor_handling(
                    &metadata,
                    &friend,
                    service.neighbor_history_generation(),
                    |history| {
                        history.entries[0]
                            .handling
                            .set(State::Stopped, Some("owner_stopped"), 2)
                    },
                )
                .await
                .unwrap();
        }
        let before = metadata.history_page(&friend, None, 50).await.unwrap();
        for kind in [Kind::Answer, Kind::Refusal] {
            store.0.store(0, Ordering::SeqCst);
            let error = service
                .send_neighbor_once(
                    &transport,
                    &lifecycle,
                    &metadata,
                    Outgoing {
                        friendship: friend.clone(),
                        kind,
                        body: OutboundBody::Granted(Zeroizing::new("opaque later reply".into())),
                        in_reply_to: Some(target.clone()),
                        author: Author::Agent {
                            agent: "concierge".into(),
                            name: "Concierge".into(),
                        },
                    },
                )
                .await
                .err()
                .unwrap();
            assert_eq!(
                error,
                if stopped {
                    Error::ReplyTargetStopped
                } else {
                    Error::SecretStore
                }
            );
            assert_eq!(store.0.load(Ordering::SeqCst), usize::from(!stopped));
            assert_eq!(
                metadata.history_page(&friend, None, 50).await.unwrap(),
                before
            );
        }
    }
}
