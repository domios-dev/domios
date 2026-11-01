use super::*;
use codeterm_neighbors::{adapter::AuthenticatedCall, wire::SignedEnvelope};
use std::sync::Mutex;

struct Mock {
    file: Manifest,
    object: Vec<u8>,
    events: Mutex<Vec<String>>,
    mismatch: bool,
}

#[async_trait]
impl Put for Mock {
    async fn put(
        &self,
        url: &str,
        headers: &serde_json::Map<String, Value>,
        object: Vec<u8>,
    ) -> codeterm_neighbors::Result<()> {
        assert_eq!(url, "https://upload.invalid/opaque");
        assert_eq!(headers["Content-Type"], "application/octet-stream");
        assert_eq!(
            headers["Content-MD5"],
            STANDARD.encode(Md5::digest(&object))
        );
        assert_eq!(
            headers["x-goog-content-length-range"],
            format!("{},{}", object.len(), object.len())
        );
        assert_eq!(headers["x-goog-if-generation-match"], "0");
        assert_eq!(object, self.object);
        self.events.lock().unwrap().push("put".into());
        Ok(())
    }
}

#[async_trait]
impl AuthenticatedCall for Mock {
    async fn id_token(&self) -> codeterm_neighbors::Result<Zeroizing<String>> {
        panic!("no network")
    }
    async fn invoke(&self, route: &str, data: Value) -> codeterm_neighbors::Result<Value> {
        self.events.lock().unwrap().push(route.into());
        assert_eq!(data["installation_id"], "machine");
        match route {
            "friendsFileGrant" => {
                assert_eq!(data["ciphertext_size"], self.object.len());
                assert_eq!(data["ciphertext_sha256"], self.file.ciphertext_sha256);
                assert_eq!(
                    data["ciphertext_md5_b64"],
                    STANDARD.encode(Md5::digest(&self.object))
                );
                assert!(data.get("key_b64u").is_none());
                assert!(data.get("name").is_none());
                Ok(
                    json!({"schema_version":1,"file_id":self.file.file_id,"object_id":self.file.object_id,"state":"granted","upload":{"method":"PUT","url":"https://upload.invalid/opaque","headers":{"Content-Type":"application/octet-stream","Content-MD5":data["ciphertext_md5_b64"],"x-goog-content-length-range":format!("{},{}",self.object.len(),self.object.len()),"x-goog-if-generation-match":"0"}}}),
                )
            }
            "friendsFileFinalize" => Ok(
                json!({"schema_version":1,"file_id":self.file.file_id,"object_id":self.file.object_id,"state":"ready","ciphertext_size":self.file.ciphertext_size,"ciphertext_sha256":if self.mismatch { "bad" } else { &self.file.ciphertext_sha256 }}),
            ),
            "friendsSend" => {
                assert_eq!(data["file_ids"], json!([self.file.file_id]));
                assert!(data.get("attachments").is_none());
                assert!(!data.to_string().contains("key_b64u"));
                Ok(json!({}))
            }
            _ => panic!("unexpected route {route}"),
        }
    }
}

#[tokio::test]
async fn file_send_grants_uploads_finalizes_then_publishes_ids() {
    let golden: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../codeterm-neighbors/test/golden-v2.json"
    )))
    .unwrap();
    let signed: SignedEnvelope = serde_json::from_value(
        json!({"envelope":golden["envelope"],"signature":golden["signature"]}),
    )
    .unwrap();
    for mismatch in [false, true] {
        let (file, object) = files::seal(
            &binding(&signed.envelope),
            &"c".repeat(32),
            "NUL.txt",
            "text/plain",
            b"opaque bytes",
            &mut rand::rngs::OsRng,
        )
        .unwrap();
        let mock = Arc::new(Mock {
            file: file.clone(),
            object: object.clone(),
            events: Mutex::default(),
            mismatch,
        });
        let transport = FirebaseTransport {
            calls: mock.clone(),
            project: "fixture".into(),
            uid: signed.envelope.sender_uid.clone(),
            installation_id: "machine".into(),
        };
        let result = upload_with(&transport, &signed.envelope, &file, object, mock.as_ref()).await;
        if mismatch {
            assert_eq!(result, Err(Error::Transport));
        } else {
            result.unwrap();
            send_envelope(&transport, &signed, &[file.file_id.clone()])
                .await
                .unwrap();
        }
        let expected = if mismatch {
            vec!["friendsFileGrant", "put", "friendsFileFinalize"]
        } else {
            vec![
                "friendsFileGrant",
                "put",
                "friendsFileFinalize",
                "friendsSend",
            ]
        };
        assert_eq!(*mock.events.lock().unwrap(), expected);
    }
}
