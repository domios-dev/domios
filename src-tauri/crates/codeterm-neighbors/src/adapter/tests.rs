use super::*;
use crate::wire::DeliveryReason;

struct Calls;

struct LimitedSend(std::sync::atomic::AtomicUsize);
#[async_trait]
impl AuthenticatedCall for LimitedSend {
    async fn invoke(&self, route: &str, data: Value) -> Result<Value> {
        assert_eq!(route, "friendsSend");
        if data["envelope"]["in_reply_to"].is_null() {
            assert!(data.get("in_reply_to").is_none());
        } else {
            assert_eq!(data["in_reply_to"], data["envelope"]["in_reply_to"]);
        }
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Err(Error::DailyLimit {
            limit: 3,
            resets_at: 1000,
        })
    }
    async fn id_token(&self) -> Result<Zeroizing<String>> {
        Err(Error::Transport)
    }
}

#[tokio::test]
async fn linked_reply_reaches_the_callable_once_and_keeps_a_daily_limit_refusal() {
    let golden: Value = serde_json::from_str(include_str!("../../test/golden-v2.json")).unwrap();
    let mut message: SignedEnvelope = serde_json::from_value(
        json!({"envelope":golden["envelope"],"signature":golden["signature"]}),
    )
    .unwrap();
    message.envelope.in_reply_to = Some("b".repeat(32));
    let calls = Arc::new(LimitedSend(0.into()));
    let transport = FirebaseTransport {
        calls: calls.clone(),
        uid: message.envelope.sender_uid.clone(),
        project: "fixture".into(),
        installation_id: "machine".into(),
    };
    assert_eq!(
        transport.send(&message).await,
        Err(Error::DailyLimit {
            limit: 3,
            resets_at: 1000
        })
    );
    assert_eq!(calls.0.load(std::sync::atomic::Ordering::Relaxed), 1);
    message.envelope.in_reply_to = None;
    assert_eq!(
        transport.send(&message).await,
        Err(Error::DailyLimit {
            limit: 3,
            resets_at: 1000
        })
    );
    assert_eq!(calls.0.load(std::sync::atomic::Ordering::Relaxed), 2);
}
#[async_trait]
impl AuthenticatedCall for Calls {
    async fn invoke(&self, route: &str, data: Value) -> Result<Value> {
        assert!(matches!(route, "friendsAck" | "friendsDeliveryStatus"));
        assert_eq!(data["installation_id"], "machine");
        Ok(json!({"status":"not_delivered","reason":"key_revoked"}))
    }
    async fn id_token(&self) -> Result<Zeroizing<String>> {
        Err(Error::Transport)
    }
}

#[tokio::test]
async fn callable_delivery_and_ack_preserve_key_revoked_reason() {
    let transport = FirebaseTransport {
        calls: Arc::new(Calls),
        project: "fixture".into(),
        uid: "owner".into(),
        installation_id: "machine".into(),
    };
    for result in [
        transport.delivery_status("message").await.unwrap(),
        transport.ack("sender", "message").await.unwrap(),
    ] {
        assert_eq!(
            result,
            DeliveryResult {
                status: DeliveryStatus::NotDelivered,
                reason: Some(DeliveryReason::KeyRevoked),
                delivered_at: None,
                read_at: None,
            }
        );
        assert_eq!(
            serde_json::to_value(result).unwrap(),
            json!({"status":"not_delivered","reason":"key_revoked"})
        );
    }
}

#[tokio::test]
async fn enrollment_registers_only_the_installation_key_without_receiver_state() {
    struct EnrollCalls;
    #[async_trait]
    impl AuthenticatedCall for EnrollCalls {
        async fn invoke(&self, route: &str, data: Value) -> Result<Value> {
            assert_eq!(route, "friendsEnrollVerifierKey");
            assert_eq!(data.as_object().unwrap().len(), 5);
            assert_eq!(data["public_key_spki_b64"], "signing");
            assert_eq!(data["encryption_public_key_spki_b64"], "encryption");
            assert_eq!(data["machine_label"], "Desk");
            assert_eq!(data["installation_id"], "machine");
            assert!(data["operation_id"].as_str().is_some());
            Ok(json!({"verifier_key_id":"vk_registered"}))
        }
        async fn id_token(&self) -> Result<Zeroizing<String>> {
            panic!("no authentication")
        }
    }
    let registry = FirebaseRegistry {
        calls: Arc::new(EnrollCalls),
        uid: "owner".into(),
        installation_id: "machine".into(),
        machine_label: "Desk".into(),
    };
    assert_eq!(
        registry.enroll("signing", "encryption").await.unwrap(),
        "vk_registered"
    );
}

#[tokio::test]
async fn uncertain_key_enrollment_is_never_replayed() {
    struct Uncertain(std::sync::atomic::AtomicUsize);
    #[async_trait]
    impl AuthenticatedCall for Uncertain {
        async fn invoke(&self, route: &str, _: Value) -> Result<Value> {
            assert_eq!(route, "friendsEnrollVerifierKey");
            self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Err(Error::Transport)
        }
        async fn id_token(&self) -> Result<Zeroizing<String>> {
            panic!("no authentication")
        }
    }
    let calls = Arc::new(Uncertain(0.into()));
    let registry = FirebaseRegistry {
        calls: calls.clone(),
        uid: "owner".into(),
        installation_id: "machine".into(),
        machine_label: "Desk".into(),
    };
    assert_eq!(
        registry.enroll("signing", "encryption").await,
        Err(Error::Transport)
    );
    assert_eq!(calls.0.load(std::sync::atomic::Ordering::Relaxed), 1);
}
