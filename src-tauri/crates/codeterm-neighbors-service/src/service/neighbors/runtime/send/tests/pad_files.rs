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
