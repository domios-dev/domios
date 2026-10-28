use super::*;
use crate::crypto::tests::{recipient, vector};
use crate::crypto::PrivateKeys;
use crate::wire::PeerKey;
use std::sync::Mutex;
mod attachments;

struct Fixture {
    peer_reads: Mutex<usize>,
    events: Mutex<Vec<&'static str>>,
    cursor: Mutex<Option<Cursor>>,
    fail_ack: Mutex<bool>,
    admitted: Mutex<bool>,
    writes: Mutex<usize>,
    uncertain: bool,
}

impl Fixture {
    fn new() -> Self {
        Self {
            peer_reads: Mutex::new(0),
            events: Mutex::new(vec![]),
            cursor: Mutex::new(None),
            fail_ack: Mutex::new(false),
            admitted: Mutex::new(false),
            writes: Mutex::new(0),
            uncertain: false,
        }
    }
    fn peers(&self) -> PeerKeys {
        let v = vector();
        PeerKeys {
            friendship_id: v["envelope"]["friendship_id"].as_str().unwrap().into(),
            peer_uid: v["envelope"]["sender_uid"].as_str().unwrap().into(),
            keys: vec![PeerKey {
                verifier_key_id: "vk_test_1".into(),
                public_key_spki_b64: v["spki"].as_str().unwrap().into(),
                encryption_public_key_spki_b64: v["senderSpki"].as_str().unwrap().into(),
                enc_key_id: v["envelope"]["enc_key_ids"]["sender"]
                    .as_str()
                    .unwrap()
                    .into(),
                created_at: 1,
            }],
        }
    }
}

#[async_trait]
impl NeighborTransport for Fixture {
    async fn send(&self, _: &SignedEnvelope) -> Result<DeliveryStatus> {
        Ok(DeliveryStatus::Pending)
    }
    async fn listen_from_cursor(
        &self,
        _: &str,
        _: Option<&Cursor>,
    ) -> Result<Box<dyn InboxListener>> {
        Err(Error::Transport)
    }
    async fn ack(&self, _: &str, _: &str) -> Result<DeliveryResult> {
        self.events.lock().unwrap().push("ack");
        if *self.fail_ack.lock().unwrap() {
            Err(Error::Transport)
        } else {
            Ok(DeliveryStatus::Delivered.into())
        }
    }
    async fn peer_keys(&self, _: &str) -> Result<PeerKeys> {
        *self.peer_reads.lock().unwrap() += 1;
        Ok(self.peers())
    }
    async fn delivery_status(&self, _: &str) -> Result<DeliveryResult> {
        Ok(DeliveryStatus::Pending.into())
    }
    fn verify<'a>(
        &self,
        doc: &'a InboxDocument,
        peers: &'a PeerKeys,
        recipient: &str,
        now: Timestamp,
    ) -> Result<&'a str> {
        self.events.lock().unwrap().push("verify");
        verify_inbound(doc, peers, recipient, now)
    }
}

#[async_trait]
impl CursorStore for Fixture {
    async fn load(&self, _: &str) -> Result<Option<Cursor>> {
        Ok(self.cursor.lock().unwrap().clone())
    }
    async fn save(&self, _: &str, cursor: &Cursor) -> Result<()> {
        self.events.lock().unwrap().push("cursor");
        *self.cursor.lock().unwrap() = Some(cursor.clone());
        Ok(())
    }
}

#[async_trait]
impl NeighborGateway for Fixture {
    async fn acknowledged(&self, _: &InboxMessage, result: DeliveryResult) -> Result<()> {
        assert_ne!(result.status,DeliveryStatus::Pending);
        self.events.lock().unwrap().push("ack_committed");
        Ok(())
    }
    async fn deliver(
        &self,
        _: &InboxMessage,
        plaintext: Zeroizing<String>,
    ) -> Result<GatewayReceipt> {
        assert_eq!(plaintext.as_str(), vector()["body"].as_str().unwrap());
        self.events.lock().unwrap().push("decrypted_gateway");
        let mut admitted = self.admitted.lock().unwrap();
        if !*admitted {
            *self.writes.lock().unwrap() += 1;
            *admitted = true;
        }
        Ok(if self.uncertain {
            GatewayReceipt::SubmitUnconfirmed
        } else {
            GatewayReceipt::Delivered
        })
    }
}

fn message() -> InboxMessage {
    let v = vector();
    let doc = InboxDocument {
        envelope: serde_json::from_value(v["envelope"].clone()).unwrap(),
        signature: v["signature"].as_str().unwrap().into(),
        received_at: Timestamp {
            seconds: 10,
            nanoseconds: 0,
        },
        expires_at: Timestamp {
            seconds: 100,
            nanoseconds: 0,
        },
    };
    InboxMessage {
        id: format!("{}_{}", doc.envelope.sender_uid, doc.envelope.message_id),
        doc,
    }
}

fn receiver<'a>(f: &'a Fixture, keys: &'a PrivateKeys, message: &'a InboxMessage) -> Receiver<'a> {
    Receiver {
        transport: f,
        cursors: f,
        gateway: f,
        private_keys: keys,
        recipient_uid: &message.doc.envelope.recipient_uid,
        friendship_id: &message.doc.envelope.friendship_id,
    }
}

const NOW: Timestamp = Timestamp {
    seconds: 50,
    nanoseconds: 0,
};

#[tokio::test]
async fn peer_keys_are_read_once_per_nonduplicate_batch_and_never_reused_across_batches() {
    let fixture = Fixture::new();
    let keys = recipient();
    let template = message();
    let vector = vector();
    let sender = crate::crypto::tests::keys(
        vector["senderSeed"].as_str().unwrap(),
        vector["seed"].as_str().unwrap(),
    );
    let message = |id| {
        let mut message = template.clone();
        message.doc.envelope.message_id = format!("{id:032x}");
        message.doc.signature = sender.sign(message.doc.envelope.clone()).unwrap().signature;
        message.id = format!(
            "{}_{}",
            message.doc.envelope.sender_uid, message.doc.envelope.message_id
        );
        message.doc.received_at.seconds += id;
        message
    };
    let mut receiver = receiver(&fixture, &keys, &template);
    assert_eq!(
        receiver
            .catch_up(&mut [message(1), message(2)], NOW)
            .await
            .unwrap(),
        vec![ReceiveOutcome::Delivered, ReceiveOutcome::Delivered]
    );
    assert_eq!(*fixture.peer_reads.lock().unwrap(), 1);
    assert_eq!(
        receiver.catch_up(&mut [message(3)], NOW).await.unwrap(),
        vec![ReceiveOutcome::Delivered]
    );
    assert_eq!(*fixture.peer_reads.lock().unwrap(), 2);
    assert_eq!(
        receiver
            .catch_up(&mut [message(1), message(2)], NOW)
            .await
            .unwrap(),
        vec![ReceiveOutcome::Duplicate, ReceiveOutcome::Duplicate]
    );
    assert_eq!(*fixture.peer_reads.lock().unwrap(), 2);
}

#[tokio::test]
async fn verified_decrypted_gateway_delivery_precedes_ack_and_cursor() {
    let f = Fixture::new();
    let keys = recipient();
    let message = message();
    let mut receiver = receiver(&f, &keys, &message);
    assert_eq!(
        receiver.process(&message, NOW).await.unwrap(),
        ReceiveOutcome::Delivered
    );
    assert_eq!(
        *f.events.lock().unwrap(),
        ["verify", "decrypted_gateway", "ack", "ack_committed", "cursor"]
    );
    assert_eq!(
        receiver.process(&message, NOW).await.unwrap(),
        ReceiveOutcome::Duplicate
    );
    assert_eq!(*f.writes.lock().unwrap(), 1);
}

#[tokio::test]
async fn invalid_expired_and_wrong_identity_messages_never_reach_gateway_or_ack() {
    let keys = recipient();
    let original = message();
    for case in 0..4 {
        let f = Fixture::new();
        let mut changed = original.clone();
        let expected = match case {
            0 => {
                changed.doc.signature.replace_range(..1, "A");
                Error::BadSignature
            }
            1 => {
                changed.doc.expires_at = NOW;
                Error::Expired
            }
            2 => {
                changed.doc.envelope.recipient_uid = "other".into();
                Error::WrongPeer
            }
            _ => {
                changed.doc.envelope.verifier_key_id = "revoked".into();
                Error::InvalidKey
            }
        };
        let mut receiver = receiver(&f, &keys, &original);
        assert_eq!(
            receiver.process(&changed, NOW).await.unwrap(),
            ReceiveOutcome::Rejected(expected)
        );
        assert_eq!(*f.events.lock().unwrap(), ["verify", "cursor"]);
        assert_eq!(*f.writes.lock().unwrap(), 0);
    }
}

#[tokio::test]
async fn failed_ack_keeps_cursor_and_gateway_dedup_prevents_rewrite() {
    let f = Fixture::new();
    let keys = recipient();
    let message = message();
    let mut receiver = receiver(&f, &keys, &message);
    *f.fail_ack.lock().unwrap() = true;
    assert_eq!(receiver.process(&message, NOW).await, Err(Error::Transport));
    assert!(f.cursor.lock().unwrap().is_none());
    assert!(!f.events.lock().unwrap().contains(&"ack_committed"));
    *f.fail_ack.lock().unwrap() = false;
    assert_eq!(
        receiver.process(&message, NOW).await.unwrap(),
        ReceiveOutcome::Delivered
    );
    assert_eq!(*f.writes.lock().unwrap(), 1);
    assert_eq!(f.events.lock().unwrap().iter().filter(|event| **event=="ack_committed").count(),1);
}

#[tokio::test]
async fn unconfirmed_gateway_submission_is_never_acknowledged() {
    let mut f = Fixture::new();
    f.uncertain = true;
    let keys = recipient();
    let message = message();
    assert_eq!(
        receiver(&f, &keys, &message).process(&message, NOW).await,
        Err(Error::Gateway)
    );
    assert_eq!(*f.events.lock().unwrap(), ["verify", "decrypted_gateway"]);
    assert!(f.cursor.lock().unwrap().is_none());
}

#[test]
fn cursor_order_includes_nanoseconds_and_document_id() {
    let base = Cursor {
        received_at: Timestamp {
            seconds: 10,
            nanoseconds: 2,
        },
        id: "a".into(),
    };
    let same_time = Cursor {
        id: "b".into(),
        ..base.clone()
    };
    let later = Cursor {
        received_at: Timestamp {
            seconds: 10,
            nanoseconds: 3,
        },
        id: "a".into(),
    };
    assert!(base < same_time && same_time < later);
}

mod recovery;
