use super::*;
use codeterm_neighbors::adapter::AuthenticatedCall;
use std::{path::PathBuf, sync::Mutex};

struct Calls {
    object: Vec<u8>,
    file: Manifest,
    folder: PathBuf,
    trace: Mutex<Vec<String>>,
}

#[async_trait]
impl AuthenticatedCall for Calls {
    async fn id_token(&self) -> codeterm_neighbors::Result<Zeroizing<String>> { panic!("no network") }
    async fn invoke(&self, route: &str, data: Value) -> codeterm_neighbors::Result<Value> {
        self.trace.lock().unwrap().push(route.into());
        assert_eq!(data["installation_id"], "machine");
        assert_eq!(data["file_id"], self.file.file_id);
        assert_eq!(data["sender_uid"], "sender");
        match route {
            "friendsFileDownload" => Ok(json!({"schema_version":1,"file_id":self.file.file_id,"generation":"1","ciphertext_size":self.file.ciphertext_size,"ciphertext_sha256":self.file.ciphertext_sha256,"ciphertext_b64url":base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&self.object)})),
            "friendsFileConfirm" => {
                assert_eq!(data["outcome"], "saved");
                let filename = storage::Entry::new(&self.file, &envelope(), "in", "saved").filename();
                assert_eq!(std::fs::read(self.folder.join(filename)).unwrap(), b"received bytes");
                Ok(json!({"schema_version":1,"file_id":self.file.file_id,"state":"deleted"}))
            }
            _ => panic!("unexpected route {route}"),
        }
    }
}

fn envelope() -> Envelope {
    let golden: Value = serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../codeterm-neighbors/test/golden-v2.json"))).unwrap();
    let mut envelope: Envelope = serde_json::from_value(golden["envelope"].clone()).unwrap();
    envelope.sender_uid = "sender".into();
    envelope.recipient_uid = "recipient".into();
    envelope
}

#[tokio::test]
async fn file_receive_writes_verified_bytes_before_server_confirm() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let service = &fixture.service;
    *service.neighbor_cell().receiver.lock() = Some(serde_json::from_value(json!({"receiver_revision":1,"receiver_installation_id":"machine","receiver_key_id":null,"receiver_machine_label":"Here","receiving_enabled":true,"is_me":true})).unwrap());
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "recipient");
    let envelope = envelope();
    let (file, object) = files::seal(&binding(&envelope), &"c".repeat(32), "NUL.txt", "text/plain", b"received bytes", &mut rand::rngs::OsRng).unwrap();
    let calls = Arc::new(Calls { file: file.clone(), object, folder: metadata.friend_folder(&envelope.friendship_id).unwrap(), trace: Mutex::default() });
    let transport = FirebaseTransport { calls: calls.clone(), project: "fixture".into(), uid: "recipient".into(), installation_id: "machine".into() };
    let ports = Ports { service, metadata: &metadata, transport: &transport, envelope: &envelope, listener_generation: 0 };
    codeterm_neighbors::files::transfer::receive(&ports, &file, &binding(&envelope)).await.unwrap();
    assert_eq!(*calls.trace.lock().unwrap(), ["friendsFileDownload", "friendsFileConfirm"]);
    let entries = metadata.file_entries(&envelope.friendship_id).await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].state, "saved");
    assert_eq!(entries[0].name, "_NUL.txt");
    let path = metadata.saved_file(&envelope.friendship_id, &file.file_id).await.unwrap();
    assert_eq!(std::fs::read(path).unwrap(), b"received bytes");
    assert!(!serde_json::to_string(&entries).unwrap().contains("key_b64u"));
}
