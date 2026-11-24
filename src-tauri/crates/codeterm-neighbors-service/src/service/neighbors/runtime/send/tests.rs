use super::*;

mod admission;
pub mod outbound_authority;
mod pad_files;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};

async fn incoming_question(
    fixture: &ServiceFixture,
    metadata: &Metadata,
    friendship: &str,
) -> String {
    let target = "b".repeat(32);
    let entry = serde_json::from_value(json!({"direction":"in","author":"friend","sender_uid":"peer","body":"opaque incoming","at":1,"message_id":target,"kind":"question"})).unwrap();
    fixture
        .service
        .append_neighbor_history(
            metadata,
            friendship,
            entry,
            fixture.service.neighbor_history_generation(),
        )
        .await
        .unwrap();
    target
}

#[tokio::test]
async fn refusal_preflight_accepts_cyrillic_and_rejects_oversize_without_writing() {
    let (mux, _) = MultiplexerState::new();
    let fixture = crate::test_support::fixture(mux);
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friendship = "a".repeat(64);
    let target = incoming_question(&fixture, &metadata, &friendship).await;
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

pub struct NoNetwork;
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
pub struct KeyFence(std::sync::atomic::AtomicUsize);
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
    let fixture = crate::test_support::fixture(MultiplexerState::new().0);
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
    let target = incoming_question(&fixture, &metadata, &friend).await;
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
                        message_id: None,
                        authority: SendAuthority::FriendshipGrant,
                        friendship: friend.clone(),
                        kind,
                        body: OutboundBody::Granted(Zeroizing::new(text)),
                        in_reply_to: (kind == Kind::Answer).then(|| target.clone()),
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
                    message_id: None,
                    authority: SendAuthority::FriendshipGrant,
                    friendship: friend.clone(),
                    kind: Kind::Question,
                    body: OutboundBody::Files {
                        pad: None,
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
    assert_eq!(
        metadata.history_page(&friend, None, 50).await.unwrap(),
        before
    );
    let existing_id = "c".repeat(32);
    let existing = serde_json::from_value(json!({"direction":"out","author":"me","sender_uid":"owner","body":"opaque retained send","at":1,"message_id":existing_id,"kind":"question"})).unwrap();
    service
        .append_neighbor_history(
            &metadata,
            &friend,
            existing,
            service.neighbor_history_generation(),
        )
        .await
        .unwrap();
    let before = metadata.history_page(&friend, None, 50).await.unwrap();
    for (message_id, expected) in [
        ("invalid".into(), Error::InvalidEnvelope),
        (existing_id, Error::OperationIdConflict),
        ("d".repeat(32), Error::SecretStore),
    ] {
        store.0.store(0, Ordering::SeqCst);
        let error = service
            .send_neighbor_once(
                &transport,
                &lifecycle,
                &metadata,
                Outgoing {
                    message_id: Some(message_id),
                    authority: SendAuthority::FriendshipGrant,
                    friendship: friend.clone(),
                    kind: Kind::Question,
                    body: OutboundBody::Granted(Zeroizing::new("opaque original send".into())),
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
                message_id: None,
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
    let fixture = crate::test_support::fixture(MultiplexerState::new().0);
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
                        message_id: None,
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

#[test]
fn send_command_preserves_client_message_identity() {
    let id = "a".repeat(32);
    let command: NeighborRuntimeCommand = serde_json::from_value(json!({
        "operation":"send","friendship_id":"b".repeat(64),"kind":"question",
        "body":"opaque message","in_reply_to":null,"message_id":id
    }))
    .unwrap();
    assert!(
        matches!(command, NeighborRuntimeCommand::Send { message_id: Some(observed), .. } if observed == id)
    );
}

#[tokio::test]
async fn sandboxed_neighbor_reply_requires_a_request_in_the_bound_friendship() {
    let (mux, _) = MultiplexerState::new();
    let fixture = crate::test_support::fixture(mux);
    let service = &fixture.service;
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let mut permission = Permission {
        allowed: true,
        machine_access: MachineAccess::ReadOnly,
        scope: "opaque grant".into(),
        ..Default::default()
    };
    let friendship = "a".repeat(64);
    let foreign = "b".repeat(64);
    let question = "a".repeat(32);
    let answer = "b".repeat(32);
    let other_question = "c".repeat(32);
    for (friend, id, kind) in [
        (&friendship, &question, "question"),
        (&friendship, &answer, "answer"),
        (&foreign, &other_question, "question"),
    ] {
        let entry = serde_json::from_value(json!({"direction":"in","author":"friend","sender_uid":"peer","body":"opaque","at":1,"message_id":id,"kind":kind})).unwrap();
        service
            .append_neighbor_history(
                &metadata,
                friend,
                entry,
                service.neighbor_history_generation(),
            )
            .await
            .unwrap();
    }
    for access in [MachineAccess::ReadOnly, MachineAccess::Folder] {
        permission.machine_access = access;
        for kind in [Kind::Answer, Kind::Refusal] {
            for target in [None, Some(answer.as_str()), Some(other_question.as_str())] {
                let result = prepare_outgoing_body(
                    &metadata,
                    &friendship,
                    &permission,
                    SendAuthority::FriendshipGrant,
                    kind,
                    OutboundBody::Granted(Zeroizing::new("opaque reply".into())),
                    target,
                )
                .await;
                if access == MachineAccess::Folder && kind == Kind::Answer && target.is_none() {
                    assert!(result.is_ok());
                } else {
                    assert!(matches!(result, Err(Error::ReplyTargetUnknown)));
                }
            }
            assert!(prepare_outgoing_body(
                &metadata,
                &friendship,
                &permission,
                SendAuthority::FriendshipGrant,
                kind,
                OutboundBody::Granted(Zeroizing::new("opaque reply".into())),
                Some(&question)
            )
            .await
            .is_ok());
        }
    }
}
