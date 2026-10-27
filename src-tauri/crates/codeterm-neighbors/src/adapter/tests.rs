use super::*;
use crate::wire::DeliveryReason;

struct Calls;

struct LimitedSend(std::sync::atomic::AtomicUsize);
#[async_trait]
impl AuthenticatedCall for LimitedSend {
    async fn invoke(&self, route: &str, data: Value) -> Result<Value> {
        assert_eq!(route, "friendsSend");
        assert_eq!(data["in_reply_to"], data["envelope"]["in_reply_to"]);
        assert_eq!(data["in_reply_to"], "b".repeat(32));
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

struct ReceiverCalls {
    calls: std::sync::Mutex<Vec<(String, Value)>>,
    uncertain: bool,
}

#[async_trait]
impl AuthenticatedCall for ReceiverCalls {
    async fn invoke(&self, route: &str, data: Value) -> Result<Value> {
        self.calls.lock().unwrap().push((route.into(), data));
        match route {
            "friendsReceiverStatus" => Ok(
                json!({"receiver_revision":7,"receiver_installation_id":"other",
                "receiver_key_id":null,"receiver_machine_label":"Other","receiving_enabled":true,"is_me":false}),
            ),
            "friendsEnrollVerifierKey" if self.uncertain => Err(Error::Transport),
            "friendsEnrollVerifierKey" => Err(Error::ReceiverRevisionStale),
            _ => panic!("unexpected callable {route}"),
        }
    }
    async fn id_token(&self) -> Result<Zeroizing<String>> {
        Err(Error::Transport)
    }
}

#[tokio::test]
async fn receiver_enrollment_requires_explicit_revision_and_never_replays_uncertainty() {
    for uncertain in [false, true] {
        let calls = Arc::new(ReceiverCalls {
            calls: Default::default(),
            uncertain,
        });
        let mut registry = FirebaseRegistry {
            calls: calls.clone(),
            uid: "owner".into(),
            installation_id: "machine".into(),
            machine_label: "Here".into(),
            enrollment: None,
        };
        assert_eq!(
            registry.enroll("signing", "encryption", false).await,
            Err(Error::NotReceiver)
        );
        assert!(calls
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|(route, _)| route != "friendsEnrollVerifierKey"));
        registry.enrollment = Some(Enrollment {
            expected_revision: 6,
            takeover: true,
        });
        assert_eq!(
            registry.enroll("signing", "encryption", true).await,
            Err(if uncertain {
                Error::Transport
            } else {
                Error::ReceiverRevisionStale
            })
        );
        let calls = calls.calls.lock().unwrap();
        let enrollments: Vec<_> = calls
            .iter()
            .filter(|(route, _)| route == "friendsEnrollVerifierKey")
            .collect();
        assert_eq!(enrollments.len(), 1);
        let data = &enrollments[0].1;
        assert_eq!(data["expected_receiver_revision"], 6);
        assert_eq!(data["installation_id"], "machine");
        assert_eq!(data["machine_label"], "Here");
        assert_eq!(data["takeover"], true);
        let operation_id = data["operation_id"].as_str().unwrap();
        assert!((16..=64).contains(&operation_id.len()));
        assert!(operation_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'));
        assert!(data.get("replace_others").is_none());
        assert_eq!(
            calls.last().unwrap().0,
            if uncertain {
                "friendsReceiverStatus"
            } else {
                "friendsEnrollVerifierKey"
            }
        );
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
                reason: Some(DeliveryReason::KeyRevoked)
            }
        );
        assert_eq!(
            serde_json::to_value(result).unwrap(),
            json!({"status":"not_delivered","reason":"key_revoked"})
        );
    }
}

#[test]
fn final_receiver_status_preserves_disabled_machine_and_accepts_legacy_metadata() {
    let status: ReceiverStatus = serde_json::from_value(json!({
        "schema_version":1,"receiver_revision":8,"receiver_installation_id":"machine",
        "receiver_key_id":null,"receiver_session_id":null,"receiver_machine_label":"Here",
        "receiving_enabled":false,"is_me":true,"legacy_active":false,"updated_at":12
    }))
    .unwrap();
    assert!(!status.receiving_here());
    assert_eq!(status.receiver_machine_label.as_deref(), Some("Here"));
    assert_eq!(status.receiver_installation_id.as_deref(), Some("machine"));
    let legacy: ReceiverStatus = serde_json::from_value(json!({
        "receiver_revision":0,"receiver_installation_id":null,"receiver_key_id":null,
        "receiver_machine_label":null,"receiving_enabled":false,"is_me":false,"legacy_active":true
    }))
    .unwrap();
    assert!(!legacy.receiving_here());
    assert_eq!(legacy.receiver_revision, 0);
}
