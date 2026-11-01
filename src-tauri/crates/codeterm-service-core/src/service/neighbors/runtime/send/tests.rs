use super::*;

mod outbound_authority;
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
    for text in ["", " \t\r\n", "\u{2003}\u{3000}"] {
        assert_eq!(
            prepare_outgoing_body(
                &metadata,
                &friendship,
                &permission,
                SendAuthority::FriendshipGrant,
                Kind::Refusal,
                OutboundBody::Granted(Zeroizing::new(text.into())),
                Some(&target)
            )
            .await
            .unwrap_err(),
            Error::EmptyBody
        );
    }
    let cyrillic = "\u{044f}".repeat(300);
    assert_eq!(cyrillic.len(), 600);
    let (accepted, paths) = prepare_outgoing_body(
        &metadata,
        &friendship,
        &permission,
        SendAuthority::FriendshipGrant,
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
            SendAuthority::FriendshipGrant,
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
                SendAuthority::FriendshipGrant,
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

    async fn id_token(&self) -> codeterm_neighbors::Result<Zeroizing<String>> {
        panic!("pre-send fence must not request a relay token")
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
async fn empty_and_oversize_sends_stop_before_keys_network_or_history_but_files_and_text_continue()
{
    use crate::service::neighbors::history::Author;
    use std::sync::{atomic::Ordering, Arc};
    let fixture = ServiceFixture::new(MultiplexerState::new().0);
    let service = &fixture.service;
    service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = true;
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friend = "a".repeat(64);
    let mut settings = crate::service::neighbors::storage::Settings::default();
    settings.permissions.insert(
        friend.clone(),
        Permission {
            allowed: true,
            allow_tab_requests: true,
            scope: "opaque grant".into(),
            ..Default::default()
        },
    );
    metadata.save_settings(settings).await.unwrap();
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
    let store = KeyFence::default();
    let lifecycle = InstallationKeys {
        store: &store,
        registry: &registry,
        scope: KeyScope::installation("machine", "owner").unwrap(),
        uid: "owner",
    };
    let before = metadata.history_page(&friend, None, 50).await.unwrap();
    for kind in [Kind::Question, Kind::Answer, Kind::TabRequest] {
        for (text, expected) in [
            (String::new(), Error::EmptyBody),
            (" \t\r\n".into(), Error::EmptyBody),
            ("\u{2003}\u{3000}\u{a0}".into(), Error::EmptyBody),
            ("x".repeat(32753), Error::MessageTooLarge { limit: 32752 }),
            ("я".repeat(16377), Error::MessageTooLarge { limit: 32752 }),
            ("x".repeat(32752), Error::SecretStore),
            ("я".repeat(16376), Error::SecretStore),
            ("opaque reply".into(), Error::SecretStore),
        ] {
            store.0.store(0, Ordering::SeqCst);
            let error = service
                .send_neighbor_once(
                    &transport,
                    &lifecycle,
                    &metadata,
                    Outgoing {
                        authority: SendAuthority::FriendshipGrant,
                        friendship: friend.clone(),
                        kind,
                        body: OutboundBody::Granted(Zeroizing::new(text)),
                        in_reply_to: None,
                        author: Author::Person("me".into()),
                    },
                )
                .await
                .err()
                .unwrap();
            assert_eq!(error, expected);
            assert_eq!(
                store.0.load(Ordering::SeqCst),
                usize::from(expected == Error::SecretStore)
            );
        }
    }
    for (paths, expected) in [
        (vec![], Error::EmptyBody),
        (vec!["opaque-file".into()], Error::SecretStore),
    ] {
        store.0.store(0, Ordering::SeqCst);
        let error = service
            .send_neighbor_once(
                &transport,
                &lifecycle,
                &metadata,
                Outgoing {
                    authority: SendAuthority::FriendshipGrant,
                    friendship: friend.clone(),
                    kind: Kind::Question,
                    body: OutboundBody::Files {
                        text: Zeroizing::new(String::new()),
                        paths,
                    },
                    in_reply_to: None,
                    author: Author::Person("me".into()),
                },
            )
            .await
            .err()
            .unwrap();
        assert_eq!(error, expected);
        assert_eq!(
            store.0.load(Ordering::SeqCst),
            usize::from(expected == Error::SecretStore)
        );
    }
    let pad = fixture.temp.path().join("failed-pad-write");
    std::fs::write(&pad, "opaque non-directory").unwrap();
    assert!(std::fs::write(pad.join("reply.txt"), "opaque answer").is_err());
    let piped = std::fs::read_to_string(pad.join("reply.txt")).unwrap_or_default();
    store.0.store(0, Ordering::SeqCst);
    let error = service
        .send_neighbor_once(
            &transport,
            &lifecycle,
            &metadata,
            Outgoing {
                authority: SendAuthority::FriendshipGrant,
                friendship: friend.clone(),
                kind: Kind::Answer,
                body: OutboundBody::Granted(Zeroizing::new(piped)),
                in_reply_to: Some("b".repeat(32)),
                author: Author::Person("me".into()),
            },
        )
        .await
        .err()
        .unwrap();
    assert_eq!(error, Error::EmptyBody);
    assert_eq!(store.0.load(Ordering::SeqCst), 0);
    assert_eq!(
        metadata.history_page(&friend, None, 50).await.unwrap(),
        before
    );
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
                        authority: SendAuthority::FriendshipGrant,
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
