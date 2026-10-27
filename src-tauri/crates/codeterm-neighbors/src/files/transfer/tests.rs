use super::*;
use crate::{files::{seal, refusal::{Failure, Reason}}, Error};
use rand::{rngs::StdRng, SeedableRng};
use std::sync::Mutex;

struct Ports {
    object: Vec<u8>,
    fail_save: bool,
    trace: Mutex<Vec<&'static str>>,
    saved: Mutex<Vec<u8>>,
}

#[async_trait]
impl ReceivePorts for Ports {
    async fn download(&self, _: &Manifest) -> Result<Vec<u8>> {
        self.trace.lock().unwrap().push("download");
        Ok(self.object.clone())
    }
    async fn verify(&self, manifest: &Manifest, binding: &Binding<'_>, object: Vec<u8>) -> Result<Zeroizing<Vec<u8>>> {
        self.trace.lock().unwrap().push("verify");
        manifest.open(binding, &object)
    }
    async fn save(&self, _: &Manifest, plaintext: Zeroizing<Vec<u8>>) -> Result<()> {
        self.trace.lock().unwrap().push("save");
        if self.fail_save { return Err(Error::CursorStore); }
        *self.saved.lock().unwrap() = plaintext.to_vec();
        Ok(())
    }
    async fn confirm(&self, _: &Manifest, outcome: &'static str) -> Result<()> {
        self.trace.lock().unwrap().push(outcome);
        if outcome == "saved" { assert_eq!(*self.saved.lock().unwrap(), b"saved bytes"); }
        Ok(())
    }
}

#[tokio::test]
async fn auto_download_saves_then_confirms_and_never_confirms_failed_save() {
    let binding = Binding { friendship_id: &"a".repeat(64), sender_uid: "s", recipient_uid: "r", message_id: &"b".repeat(32) };
    let (manifest, object) = seal(&binding, &"c".repeat(32), "report.txt", "text/plain", b"saved bytes", &mut StdRng::seed_from_u64(1)).unwrap();
    let ports = Ports { object: object.clone(), fail_save: false, trace: Mutex::default(), saved: Mutex::default() };
    receive(&ports, &manifest, &binding).await.unwrap();
    assert_eq!(*ports.trace.lock().unwrap(), ["download", "verify", "save", "saved"]);
    let ports = Ports { object, fail_save: true, trace: Mutex::default(), saved: Mutex::default() };
    assert_eq!(receive(&ports, &manifest, &binding).await, Err(Error::CursorStore));
    assert_eq!(*ports.trace.lock().unwrap(), ["download", "verify", "save"]);
}

#[tokio::test]
async fn auto_download_rejects_tamper_without_saving() {
    let binding = Binding { friendship_id: &"a".repeat(64), sender_uid: "s", recipient_uid: "r", message_id: &"b".repeat(32) };
    let (manifest, mut object) = seal(&binding, &"c".repeat(32), "x", "", b"saved bytes", &mut StdRng::seed_from_u64(1)).unwrap();
    object[16] ^= 1;
    let ports = Ports { object, fail_save: false, trace: Mutex::default(), saved: Mutex::default() };
    assert_eq!(receive(&ports, &manifest, &binding).await, Err(Error::Decryption));
    assert_eq!(*ports.trace.lock().unwrap(), ["download", "verify", "rejected"]);
    assert!(ports.saved.lock().unwrap().is_empty());
}

#[test]
fn file_refusal_preserves_server_quota_details() {
    let failure = Failure::from_callable(&serde_json::json!({"error":{"details":{"reason":"sender_quota","limit_bytes":52428800,"limit_files":25,"resets_at":1234}}})).unwrap();
    assert_eq!(failure.reason, Reason::SenderQuota);
    assert_eq!(failure.limit_files, Some(25));
    assert_eq!(failure.to_string(), "sender_quota: {\"reason\":\"sender_quota\",\"limit_bytes\":52428800,\"limit_files\":25,\"resets_at\":1234}");
    assert!(Failure::from_callable(&serde_json::json!({"error":{"details":{"reason":"unknown"}}})).is_none());
}
