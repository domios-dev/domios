use super::*;
use codeterm_neighbors::crypto::PrivateKeys;

struct NotReceiving {
    verifier: String,
    calls: parking_lot::Mutex<Vec<String>>,
}

#[async_trait::async_trait]
impl codeterm_neighbors::adapter::AuthenticatedCall for NotReceiving {
    async fn id_token(&self) -> codeterm_neighbors::Result<Zeroizing<String>> {
        panic!("fixture must not authenticate")
    }

    async fn invoke(&self, route: &str, _: Value) -> codeterm_neighbors::Result<Value> {
        self.calls.lock().push(route.into());
        match route {
            "friendsEnrollVerifierKey" => Ok(json!({"verifier_key_id":self.verifier})),
            "friendsOwnKeys" => Ok(json!([{"verifier_key_id":self.verifier}])),
            "friendsPeerKeys" => Ok(json!({
                "friendship_id":"a".repeat(64),"peer_uid":"peer","keys":[],
                "peer_receiver":{"enc_key_id":null,"revision":1,"receiving":false},
                "my_receiver":{"installation_id":"machine","enc_key_id":"opaque-key","machine_label":null,"revision":1,"receiving":true}
            })),
            _ => panic!("non-receiving recipient reached upload or submission: {route}"),
        }
    }
}

#[tokio::test]
async fn pad_file_send_to_non_receiving_recipient_never_uploads_or_writes_history() {
    let fixture = crate::test_support::fixture(MultiplexerState::new().0);
    fixture
        .service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = true;
    let pad = fixture.service.ensure_agent_pad("sender").unwrap();
    std::fs::write(pad.path().join("report.bin"), b"opaque payload").unwrap();
    let before = pad.list().unwrap();
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    use rand::SeedableRng;
    let mut rng = rand::rngs::StdRng::seed_from_u64(43);
    let keys = PrivateKeys::generate(&mut rng.clone());
    let calls = Arc::new(NotReceiving {
        verifier: keys.verifier_key_id("owner"),
        calls: Default::default(),
    });
    let transport = FirebaseTransport {
        calls: calls.clone(),
        project: "fixture".into(),
        uid: "owner".into(),
        installation_id: "machine".into(),
    };
    let registry = FirebaseRegistry {
        calls: calls.clone(),
        uid: "owner".into(),
        installation_id: "machine".into(),
        machine_label: "Fixture".into(),
    };
    let store = crate::service::neighbors::runtime::key_store::MemoryKeyStore::default();
    let scope = KeyScope::installation("machine", "owner").unwrap();
    let lifecycle = InstallationKeys {
        store: &store,
        registry: &registry,
        scope,
        uid: "owner",
    };
    lifecycle.enroll(&mut rng).await.unwrap();
    calls.calls.lock().clear();
    let error = fixture
        .service
        .send_neighbor_once(
            &transport,
            &lifecycle,
            &metadata,
            Outgoing {
                message_id: None,
                authority: SendAuthority::Unrestricted,
                friendship: "a".repeat(64),
                kind: Kind::Question,
                body: OutboundBody::Files {
                    text: Zeroizing::new(String::new()),
                    paths: vec!["report.bin".into()],
                    pad: Some(pad.clone()),
                },
                in_reply_to: None,
                author: crate::service::neighbors::history::Author::Person("me".into()),
            },
        )
        .await
        .err()
        .unwrap();
    assert_eq!(error, Error::RecipientNotReceiving);
    assert_eq!(*calls.calls.lock(), ["friendsOwnKeys", "friendsPeerKeys"]);
    assert!(metadata
        .history(&"a".repeat(64))
        .await
        .unwrap()
        .entries
        .is_empty());
    assert!(metadata
        .file_entries(&"a".repeat(64))
        .await
        .unwrap()
        .is_empty());
    assert_eq!(pad.list().unwrap(), before);
}

struct ReplyRelay {
    sender: String,
    peer: PrivateKeys,
    sent: parking_lot::Mutex<Vec<Value>>,
}

#[async_trait::async_trait]
impl codeterm_neighbors::adapter::AuthenticatedCall for ReplyRelay {
    async fn id_token(&self) -> codeterm_neighbors::Result<Zeroizing<String>> {
        panic!("fixture must not authenticate")
    }

    async fn invoke(&self, route: &str, request: Value) -> codeterm_neighbors::Result<Value> {
        match route {
            "friendsEnrollVerifierKey" => Ok(json!({"verifier_key_id":self.sender})),
            "friendsOwnKeys" => Ok(json!([{"verifier_key_id":self.sender}])),
            "friendsPeerKeys" => Ok(json!({
                "friendship_id":"a".repeat(64),"peer_uid":"peer",
                "keys":[{
                    "verifier_key_id":self.peer.verifier_key_id("peer"),
                    "public_key_spki_b64":self.peer.signing_spki(),
                    "encryption_public_key_spki_b64":self.peer.encryption_spki(),
                    "enc_key_id":self.peer.encryption_key_id(),"created_at":1
                }],
                "peer_receiver":{"enc_key_id":self.peer.encryption_key_id(),"revision":1,"receiving":true},
                "my_receiver":{"installation_id":"machine","enc_key_id":"sender-key","machine_label":null,"revision":1,"receiving":true}
            })),
            "friendsSend" => {
                self.sent.lock().push(request);
                Ok(json!({}))
            }
            _ => panic!("unexpected reply route: {route}"),
        }
    }
}

#[tokio::test]
async fn read_only_concierge_pad_body_is_accepted_as_one_linked_refusal() {
    use crate::service::neighbors::history::{handling::State, Author};
    use codeterm_neighbors::permissions::MachineAccess;
    use rand::SeedableRng;

    let (mux, tab) = MultiplexerState::new();
    let fixture = crate::test_support::fixture(mux);
    let service = &fixture.service;
    service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = true;
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friendship = "a".repeat(64);
    let question = "b".repeat(32);
    let mut settings = crate::service::neighbors::storage::Settings::default();
    settings.permissions.insert(
        friendship.clone(),
        Permission {
            allowed: true,
            machine_access: MachineAccess::ReadOnly,
            scope: "opaque grant".into(),
            ..Default::default()
        },
    );
    metadata.save_settings(settings).await.unwrap();
    let incoming = serde_json::from_value(json!({
        "direction":"in","author":"friend","sender_uid":"peer","body":"opaque question",
        "at":1,"message_id":question,"kind":"question","handling_state":"delivered_to_concierge"
    }))
    .unwrap();
    let generation = service.neighbor_history_generation();
    service
        .append_neighbor_history(&metadata, &friendship, incoming, generation)
        .await
        .unwrap();
    let pad = service.ensure_agent_pad(&tab).unwrap();
    let file = pad.path().join("refusal.txt");
    let text = "opaque refusal";
    std::fs::write(&file, text).unwrap();
    let authored = std::fs::read_to_string(file).unwrap();

    let mut rng = rand::rngs::StdRng::seed_from_u64(43);
    let keys = PrivateKeys::generate(&mut rng.clone());
    let calls = Arc::new(ReplyRelay {
        sender: keys.verifier_key_id("owner"),
        peer: PrivateKeys::generate(&mut rand::rngs::StdRng::seed_from_u64(44)),
        sent: Default::default(),
    });
    let transport = FirebaseTransport {
        calls: calls.clone(),
        project: "fixture".into(),
        uid: "owner".into(),
        installation_id: "machine".into(),
    };
    let registry = FirebaseRegistry {
        calls: calls.clone(),
        uid: "owner".into(),
        installation_id: "machine".into(),
        machine_label: "Fixture".into(),
    };
    let store = crate::service::neighbors::runtime::key_store::MemoryKeyStore::default();
    let lifecycle = InstallationKeys {
        store: &store,
        registry: &registry,
        scope: KeyScope::installation("machine", "owner").unwrap(),
        uid: "owner",
    };
    lifecycle.enroll(&mut rng).await.unwrap();
    let result = service
        .send_neighbor_once(
            &transport,
            &lifecycle,
            &metadata,
            Outgoing {
                message_id: None,
                authority: SendAuthority::FriendshipGrant,
                friendship: friendship.clone(),
                kind: Kind::Refusal,
                body: OutboundBody::Granted(Zeroizing::new(authored)),
                in_reply_to: Some(question.clone()),
                author: Author::Agent {
                    agent: tab,
                    name: "Concierge".into(),
                },
            },
        )
        .await
        .unwrap();
    let history = metadata.history(&friendship).await.unwrap();
    assert_eq!(history.entries.len(), 2);
    assert_eq!(
        history.entries[0].handling.handling_state,
        Some(State::Refused)
    );
    assert_eq!(history.entries[1].body, text);
    assert_eq!(
        history.entries[1].in_reply_to.as_deref(),
        Some(question.as_str())
    );
    let sent = calls.sent.lock();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["in_reply_to"], question);
    assert_eq!(sent[0]["envelope"]["kind"], "refusal");
    assert_eq!(
        sent[0]["envelope"]["message_id"],
        result.receipt["message_id"]
    );
}
