use super::*;
use crate::{
    crypto::tests::{keys, recipient, vector},
    files::{AttachmentBody, Binding},
    receive::{
        CursorStore, GatewayReceipt, NeighborGateway, NeighborTransport, ReceiveOutcome, Receiver,
    },
    wire::{DeliveryStatus, Envelope, PeerKey, PeerKeys, SignedEnvelope, MAX_PLAINTEXT_BYTES},
};
use serde_json::{json as json_value, Value};
use std::sync::{Arc, Mutex};

struct Relay {
    sent: Mutex<Option<Value>>,
    cursor: Mutex<Option<Cursor>>,
    events: Mutex<Vec<&'static str>>,
    peers: PeerKeys,
    body: String,
}

#[async_trait]
impl AuthenticatedCall for Relay {
    async fn id_token(&self) -> Result<Zeroizing<String>> {
        panic!("no network")
    }

    async fn invoke(&self, route: &str, data: Value) -> Result<Value> {
        match route {
            "friendsSend" => {
                self.events.lock().unwrap().push("send");
                assert_eq!(data["installation_id"], "sender-installation");
                *self.sent.lock().unwrap() = Some(data);
                Ok(json_value!({}))
            }
            "friendsPeerKeys" => Ok(serde_json::to_value(&self.peers).unwrap()),
            "friendsAck" => {
                self.events.lock().unwrap().push("ack");
                Ok(json_value!({"status":"delivered"}))
            }
            _ => panic!("unexpected route {route}"),
        }
    }
}

#[async_trait]
impl CursorStore for Relay {
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
impl NeighborGateway for Relay {
    async fn deliver(
        &self,
        message: &InboxMessage,
        plaintext: Zeroizing<String>,
    ) -> Result<GatewayReceipt> {
        let envelope = &message.doc.envelope;
        assert!(AttachmentBody::parse(
            &plaintext,
            &Binding {
                friendship_id: &envelope.friendship_id,
                sender_uid: &envelope.sender_uid,
                recipient_uid: &envelope.recipient_uid,
                message_id: &envelope.message_id,
            }
        )
        .unwrap()
        .is_none());
        assert_eq!(plaintext.as_str(), self.body);
        self.events.lock().unwrap().push("body_parsed");
        Ok(GatewayReceipt::Delivered)
    }
}

fn firestore_value(input: Value) -> fs::Value {
    use fs::value::ValueType;
    value(match input {
        Value::String(text) => ValueType::StringValue(text),
        Value::Number(number) => {
            ValueType::IntegerValue(number.as_u64().unwrap().try_into().unwrap())
        }
        Value::Null => ValueType::NullValue(0),
        Value::Object(fields) => ValueType::MapValue(fs::MapValue {
            fields: fields
                .into_iter()
                .map(|(key, value)| (key, firestore_value(value)))
                .collect(),
        }),
        _ => panic!("unexpected envelope value"),
    })
}

#[tokio::test]
async fn text_size_boundaries_round_trip_send_firestore_receive_and_body_parse() {
    let v = vector();
    let sender = keys(
        v["senderSeed"].as_str().unwrap(),
        v["seed"].as_str().unwrap(),
    );
    let recipient = recipient();
    for size in [
        MAX_PLAINTEXT_BYTES - 1,
        MAX_PLAINTEXT_BYTES,
        MAX_PLAINTEXT_BYTES + 1,
    ] {
        let body = format!(
            "{}{}",
            "é".repeat(size / 2),
            if size % 2 == 0 { "" } else { "x" }
        );
        assert_eq!(body.len(), size);
        let mut envelope: Envelope = serde_json::from_value(v["envelope"].clone()).unwrap();
        let relay = Arc::new(Relay {
            sent: Mutex::default(),
            cursor: Mutex::default(),
            events: Mutex::default(),
            peers: PeerKeys {
                peer_receiver: None,
                my_receiver: None,
                friendship_id: envelope.friendship_id.clone(),
                peer_uid: envelope.sender_uid.clone(),
                keys: vec![PeerKey {
                    verifier_key_id: envelope.verifier_key_id.clone(),
                    public_key_spki_b64: sender.signing_spki(),
                    encryption_public_key_spki_b64: sender.encryption_spki(),
                    enc_key_id: sender.encryption_key_id(),
                    created_at: 1,
                }],
            },
            body: body.clone(),
        });
        let encrypted = sender.encrypt(
            &mut envelope,
            &recipient.encryption_spki(),
            &body,
            &mut rand::rngs::OsRng,
        );
        if size > MAX_PLAINTEXT_BYTES {
            assert_eq!(
                encrypted,
                Err(Error::MessageTooLarge {
                    limit: MAX_PLAINTEXT_BYTES as u64
                })
            );
            assert!(relay.sent.lock().unwrap().is_none());
            assert!(relay.events.lock().unwrap().is_empty());
            continue;
        }
        encrypted.unwrap();
        let outbound = FirebaseTransport {
            calls: relay.clone(),
            project: "fixture".into(),
            uid: envelope.sender_uid.clone(),
            installation_id: "sender-installation".into(),
        };
        let signed = sender.sign(envelope).unwrap();
        assert_eq!(
            outbound.send(&signed).await.unwrap(),
            DeliveryStatus::Pending
        );
        let sent = relay.sent.lock().unwrap().take().unwrap();
        let round_trip: SignedEnvelope = serde_json::from_value(
            json_value!({"envelope":sent["envelope"],"signature":sent["signature"]}),
        )
        .unwrap();
        round_trip.envelope.validate().unwrap();
        let parent = "projects/fixture/databases/(default)/documents/friendInbox/recipient";
        let id = format!(
            "{}_{}",
            round_trip.envelope.sender_uid, round_trip.envelope.message_id
        );
        let timestamp = |seconds| {
            value(fs::value::ValueType::TimestampValue(
                prost_types::Timestamp { seconds, nanos: 0 },
            ))
        };
        let message = parse_document(
            fs::Document {
                name: format!("{parent}/messages/{id}"),
                fields: [
                    ("envelope".into(), firestore_value(sent["envelope"].clone())),
                    (
                        "signature".into(),
                        firestore_value(sent["signature"].clone()),
                    ),
                    ("received_at".into(), timestamp(10)),
                    ("expiresAt".into(), timestamp(100)),
                ]
                .into_iter()
                .collect(),
                ..Default::default()
            },
            parent,
        )
        .unwrap();
        assert_eq!(message.doc.envelope, round_trip.envelope);
        let inbound = FirebaseTransport {
            calls: relay.clone(),
            project: "fixture".into(),
            uid: message.doc.envelope.recipient_uid.clone(),
            installation_id: "recipient-installation".into(),
        };
        let outcome = Receiver {
            transport: &inbound,
            cursors: relay.as_ref(),
            gateway: relay.as_ref(),
            private_keys: &recipient,
            recipient_uid: &message.doc.envelope.recipient_uid,
            friendship_id: &message.doc.envelope.friendship_id,
        }
        .process(
            &message,
            Timestamp {
                seconds: 50,
                nanoseconds: 0,
            },
        )
        .await
        .unwrap();
        assert_eq!(outcome, ReceiveOutcome::Delivered);
        assert_eq!(
            *relay.events.lock().unwrap(),
            ["send", "body_parsed", "ack", "cursor"]
        );
    }
}

#[test]
fn firestore_string_bound_matches_the_wire_ciphertext_bound() {
    assert_eq!(MAX_ENVELOPE_STRING_BYTES, 43_691);
    assert!(json(
        value(fs::value::ValueType::StringValue(
            "x".repeat(MAX_ENVELOPE_STRING_BYTES)
        )),
        0
    )
    .is_ok());
    assert_eq!(
        json(
            value(fs::value::ValueType::StringValue(
                "x".repeat(MAX_ENVELOPE_STRING_BYTES + 1)
            )),
            0
        ),
        Err(Error::InvalidEnvelope)
    );
}
