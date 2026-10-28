use super::*;
use codeterm_neighbors::adapter::AuthenticatedCall;
use std::{path::PathBuf, sync::Mutex};

struct Calls {
    metadata: Metadata,
    object: Vec<u8>,
    file: Manifest,
    folder: PathBuf,
    trace: Mutex<Vec<String>>,
}

#[tokio::test]
async fn moved_friend_files_are_never_downloaded_saved_or_confirmed_by_the_old_receiver() {
    let fixture = crate::service::test_fixture::ServiceFixture::new(crate::multiplexer::state::MultiplexerState::new().0);
    let service = &fixture.service;
    let metadata = Metadata::new(fixture.temp.path().to_owned(),"recipient");
    let envelope = envelope();
    service.set_neighbor_friend_receiver_for_test(&envelope.friendship_id,Some(false),2);
    let (file,object) = files::seal(&binding(&envelope),&"c".repeat(32),"note.txt","text/plain",b"received bytes",&mut rand::rngs::OsRng).unwrap();
    let calls = Arc::new(Calls { metadata:metadata.clone(),file:file.clone(),object,
        folder:metadata.friend_folder(&envelope.friendship_id).unwrap(),trace:Mutex::default() });
    let transport = FirebaseTransport { calls:calls.clone(),project:"fixture".into(),uid:"recipient".into(),installation_id:"machine".into() };
    let ports = Ports { service,metadata:&metadata,transport:&transport,envelope:&envelope,listener_generation:0 };
    assert_eq!(codeterm_neighbors::files::transfer::receive(&ports,&file,&binding(&envelope)).await,Err(Error::NotReceiver));
    assert!(calls.trace.lock().unwrap().is_empty());
    assert!(metadata.file_entries(&envelope.friendship_id).await.unwrap().is_empty());
}

#[async_trait]
impl AuthenticatedCall for Calls {
    async fn id_token(&self) -> codeterm_neighbors::Result<Zeroizing<String>> {
        panic!("no network")
    }
    async fn invoke(&self, route: &str, data: Value) -> codeterm_neighbors::Result<Value> {
        self.trace.lock().unwrap().push(route.into());
        assert_eq!(data["installation_id"], "machine");
        if route == "friendsAck" {
            assert_eq!(data["sender_uid"], "sender");
            assert_eq!(data["message_id"], envelope().message_id);
            return Ok(json!({"status":"delivered","delivered_at":1000}));
        }
        if route == "friendsRead" {
            let envelope = envelope();
            assert_eq!(data["friendship_id"], envelope.friendship_id);
            assert_eq!(data["through_message_id"], envelope.message_id);
            let saved = self.metadata.history(&envelope.friendship_id).await?;
            assert!(saved.acknowledged_messages.contains(&envelope.message_id));
            assert_eq!(
                saved.read_receipt_through.as_deref(),
                Some(envelope.message_id.as_str())
            );
            return Ok(
                json!({"schema_version":1,"friendship_id":envelope.friendship_id,"through_message_id":envelope.message_id,"read_at":2000}),
            );
        }
        assert_eq!(data["file_id"], self.file.file_id);
        assert_eq!(data["sender_uid"], "sender");
        match route {
            "friendsFileDownload" => Ok(
                json!({"schema_version":1,"file_id":self.file.file_id,"generation":"1","ciphertext_size":self.file.ciphertext_size,"ciphertext_sha256":self.file.ciphertext_sha256,"ciphertext_b64url":base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&self.object)}),
            ),
            "friendsFileConfirm" => {
                assert_eq!(data["outcome"], "saved");
                let filename =
                    storage::Entry::new(&self.file, &envelope(), "in", "saved").filename();
                assert_eq!(
                    std::fs::read(self.folder.join(filename)).unwrap(),
                    b"received bytes"
                );
                Ok(json!({"schema_version":1,"file_id":self.file.file_id,"state":"deleted"}))
            }
            _ => panic!("unexpected route {route}"),
        }
    }
}

fn envelope() -> Envelope {
    let golden: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../codeterm-neighbors/test/golden-v2.json"
    )))
    .unwrap();
    let mut envelope: Envelope = serde_json::from_value(golden["envelope"].clone()).unwrap();
    envelope.sender_uid = "sender".into();
    envelope.recipient_uid = "recipient".into();
    envelope
}

#[tokio::test]
async fn file_bearing_answer_saves_confirms_acks_and_commits_proof_before_read() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let service = &fixture.service;
    *service.neighbor_cell().receiver.lock() = Some(serde_json::from_value(json!({"receiver_revision":1,"receiver_installation_id":"machine","receiver_key_id":null,"receiver_machine_label":"Here","receiving_enabled":true,"is_me":true})).unwrap());
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "recipient");
    let mut envelope = envelope();
    service.set_neighbor_friend_receiver_for_test(&envelope.friendship_id,Some(true),1);
    envelope.kind = Kind::Answer;
    envelope.in_reply_to = Some("f".repeat(32));
    let (file, object) = files::seal(
        &binding(&envelope),
        &"c".repeat(32),
        "NUL.txt",
        "text/plain",
        b"received bytes",
        &mut rand::rngs::OsRng,
    )
    .unwrap();
    let calls = Arc::new(Calls {
        metadata: metadata.clone(),
        file: file.clone(),
        object,
        folder: metadata.friend_folder(&envelope.friendship_id).unwrap(),
        trace: Mutex::default(),
    });
    let transport = FirebaseTransport {
        calls: calls.clone(),
        project: "fixture".into(),
        uid: "recipient".into(),
        installation_id: "machine".into(),
    };
    let ports = Ports {
        service,
        metadata: &metadata,
        transport: &transport,
        envelope: &envelope,
        listener_generation: 0,
    };
    codeterm_neighbors::files::transfer::receive(&ports, &file, &binding(&envelope))
        .await
        .unwrap();
    assert_eq!(
        *calls.trace.lock().unwrap(),
        ["friendsFileDownload", "friendsFileConfirm"]
    );
    let entries = metadata
        .file_entries(&envelope.friendship_id)
        .await
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].state, "saved");
    assert_eq!(entries[0].name, "_NUL.txt");
    let path = metadata
        .saved_file(&envelope.friendship_id, &entries[0])
        .await
        .unwrap();
    assert_eq!(std::fs::read(path).unwrap(), b"received bytes");
    assert!(!serde_json::to_string(&entries)
        .unwrap()
        .contains("key_b64u"));

    let body = AttachmentBody::new("opaque answer".into(), vec![file]);
    let encoded = body.encode(&binding(&envelope)).unwrap();
    let visible = AttachmentBody::parse(&encoded, &binding(&envelope))
        .unwrap()
        .unwrap()
        .agent_text();
    let message = InboxMessage {
        id: format!("{}_{}", envelope.sender_uid, envelope.message_id),
        doc: codeterm_neighbors::wire::InboxDocument {
            envelope: envelope.clone(),
            signature: "already-verified".into(),
            received_at: Timestamp {
                seconds: 1,
                nanoseconds: 0,
            },
            expires_at: Timestamp {
                seconds: 100,
                nanoseconds: 0,
            },
        },
    };
    let generation = service.neighbor_history_generation();
    service
        .append_neighbor_history(
            &metadata,
            &envelope.friendship_id,
            crate::service::neighbors::handling::incoming(&message, &visible, None),
            generation,
        )
        .await
        .unwrap();
    let mut before = metadata.history(&envelope.friendship_id).await.unwrap();
    assert!(!before.reserve_read_through(&envelope.message_id, true, false));
    let (_stop, stopped) = tokio::sync::watch::channel(false);
    let gateway = crate::service::neighbors::runtime::gateway::Gateway {
        service: service.clone(),
        metadata: metadata.clone(),
        friendship: envelope.friendship_id.clone(),
        tab: String::new(),
        reply_target: None,
        generation: 0,
        history_generation: generation,
        stopped,
    };
    codeterm_neighbors::receive::acknowledge_inbound(&transport, &gateway, &message)
        .await
        .unwrap();
    let mut acknowledged = metadata.history(&envelope.friendship_id).await.unwrap();
    assert!(acknowledged
        .acknowledged_messages
        .contains(&envelope.message_id));
    assert!(acknowledged.request_read_through(&envelope.message_id, true, false));
    assert!(acknowledged.reserve_read_through(&envelope.message_id, true, false));
    metadata
        .write(
            format!("history-{}.json", envelope.friendship_id),
            acknowledged,
        )
        .await
        .unwrap();
    transport.calls.invoke("friendsRead",json!({"installation_id":"machine","friendship_id":envelope.friendship_id,"through_message_id":envelope.message_id})).await.unwrap();
    assert_eq!(
        *calls.trace.lock().unwrap(),
        [
            "friendsFileDownload",
            "friendsFileConfirm",
            "friendsAck",
            "friendsRead"
        ]
    );
}
