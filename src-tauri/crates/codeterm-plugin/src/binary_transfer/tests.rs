use super::{model::*, network::*, policy::*, upload::*, worker::*};
use async_trait::async_trait;
use futures_util::{stream, StreamExt};
use parking_lot::Mutex;
use reqwest::{header::HeaderMap, Url};
use std::{
    collections::{BTreeMap, VecDeque},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
#[path = "tests/authority.rs"]
mod authority;
#[path = "tests/confinement.rs"]
mod confinement;
#[path = "tests/failure.rs"]
mod failure;
#[path = "tests/response.rs"]
mod response;

pub(crate) struct Authority {
    pub hosts: Vec<String>,
    pub allowed: AtomicBool,
}
impl TransferAuthority for Authority {
    fn authorize(&self, url: &Url) -> Result<()> {
        if self.allowed.load(Ordering::Acquire) && exact_grant(url, &self.hosts) {
            Ok(())
        } else {
            Err(TransferRefusal::NotAllowed)
        }
    }
    fn credentials(&self, _: &Credential) -> Result<BTreeMap<String, String>> {
        Ok(BTreeMap::from([(
            "authorization".into(),
            "Bearer opaque-secret".into(),
        )]))
    }
    fn readable(&self, _: &Path) -> bool {
        true
    }
}
#[derive(Clone)]
pub(crate) struct Reply {
    pub status: u16,
    pub mime: String,
    pub location: Option<String>,
    pub bytes: Vec<u8>,
    pub length: Option<u64>,
    pub failure: Option<TransferRefusal>,
}
impl Reply {
    pub fn bytes(bytes: Vec<u8>) -> Self {
        Self {
            status: 200,
            mime: "application/octet-stream".into(),
            location: None,
            length: Some(bytes.len() as u64),
            bytes,
            failure: None,
        }
    }
}
#[derive(Default)]
pub(crate) struct Network {
    pub replies: Mutex<VecDeque<Reply>>,
    pub calls: Mutex<Vec<(String, HeaderMap, Vec<u8>)>>,
}
#[async_trait]
impl TransferNetwork for Network {
    async fn request(
        &self,
        url: &Url,
        _: &str,
        headers: HeaderMap,
        mut body: Upload,
    ) -> Result<NetworkResponse> {
        let mut uploaded = Vec::new();
        while let Some(chunk) = body.stream.next().await {
            let chunk = chunk?;
            assert!(chunk.len() <= CHUNK);
            uploaded.extend_from_slice(&chunk);
        }
        assert_eq!(uploaded.len() as u64, body.length);
        self.calls
            .lock()
            .push((url.as_str().into(), headers, uploaded));
        let reply = self.replies.lock().pop_front().expect("injected reply");
        let mut chunks = reply
            .bytes
            .chunks(8191)
            .map(|chunk| Ok(chunk.to_vec()))
            .collect::<Vec<_>>();
        if let Some(error) = reply.failure {
            chunks.push(Err(error));
        }
        Ok(NetworkResponse {
            status: reply.status,
            content_type: Some(reply.mime),
            location: reply.location,
            length: reply.length,
            stream: stream::iter(chunks).boxed(),
            upload: Some(body.state.clone()),
        })
    }
}
pub(super) fn fixture(
    replies: Vec<Reply>,
) -> (
    tempfile::TempDir,
    Arc<TransferSession>,
    Arc<Authority>,
    Arc<Network>,
) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("plugin");
    std::fs::create_dir(&root).unwrap();
    let authority = Arc::new(Authority {
        hosts: vec!["api.example".into(), "cdn.example".into()],
        allowed: AtomicBool::new(true),
    });
    let session = TransferSession::at_root(
        &format!("transfer-{}", uuid::Uuid::new_v4().simple()),
        root,
        BinaryTransferDescriptor {
            credential_origins: BTreeMap::from([(
                "secret:key".into(),
                vec!["https://api.example".into()],
            )]),
            ..Default::default()
        },
        authority.clone(),
    )
    .unwrap();
    let network = Arc::new(Network {
        replies: Mutex::new(replies.into()),
        calls: Mutex::new(Vec::new()),
    });
    *session.test_network.lock() = Some(network.clone());
    (directory, session, authority, network)
}
fn request() -> TransferRequest {
    serde_json::from_value(serde_json::json!({"url":"https://api.example/output","method":"GET","response":{"kind":"file","statuses":[200],"fileName":"CON.exe","contentType":"application/octet-stream","maxBytes":1024*1024}})).unwrap()
}
fn data() -> Vec<u8> {
    (0..131_071).map(|index| (index % 256) as u8).collect()
}
fn entries(path: &Path) -> Vec<String> {
    std::fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}
#[test]
fn native_download_is_byte_exact_and_incrementally_hashed_in_a_safe_owned_file() {
    use sha2::{Digest, Sha256};
    let expected = data();
    let (_directory, session, _, network) = fixture(vec![Reply::bytes(expected.clone())]);
    let receipt = execute(session.clone(), request(), TransferAbort::default()).unwrap();
    assert_eq!(
        std::fs::read(session.root.join(&receipt.file().unwrap().path)).unwrap(),
        expected
    );
    assert_eq!(receipt.file().unwrap().size, expected.len() as u64);
    assert_eq!(
        receipt.file().unwrap().sha256,
        format!("{:x}", Sha256::digest(&expected))
    );
    assert!(receipt.file().unwrap().path.ends_with("-media.bin"));
    assert_eq!(network.calls.lock().len(), 1);
    assert_eq!(
        entries(&session.root),
        vec![receipt.file().unwrap().path.clone()]
    );
}
#[test]
fn raw_and_multipart_uploads_preserve_all_byte_values_without_text_decoding() {
    let expected = data();
    let (_directory, session, _, network) =
        fixture(vec![Reply::bytes(vec![0, 255]), Reply::bytes(vec![0, 255])]);
    std::fs::write(session.root.join("source.bin"), &expected).unwrap();
    let input = Input::Owned {
        identity: None,
        path: "source.bin".into(),
        content_type: "application/octet-stream".into(),
        file_name: "Source.bin".into(),
    };
    let mut raw = request();
    raw.method = "POST".into();
    raw.body = Some(InputBody::File {
        file: input.clone(),
    });
    execute(session.clone(), raw, TransferAbort::default()).unwrap();
    let mut multipart = request();
    multipart.method = "POST".into();
    multipart.body = Some(InputBody::Multipart {
        parts: vec![
            Part::Text {
                name: "model".into(),
                value: "opaque-model".into(),
            },
            Part::File {
                name: "reference".into(),
                file: input,
            },
        ],
    });
    execute(session, multipart, TransferAbort::default()).unwrap();
    let calls = network.calls.lock();
    assert_eq!(calls[0].2, expected);
    assert!(calls[1]
        .2
        .windows(expected.len())
        .any(|window| window == expected));
    assert!(calls[1]
        .2
        .windows(br#"name="reference""#.len())
        .any(|window| window == br#"name="reference""#));
}
