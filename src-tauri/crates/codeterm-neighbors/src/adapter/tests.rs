use super::*;
use crate::wire::DeliveryReason;

struct Calls;
#[async_trait]
impl AuthenticatedCall for Calls {
    async fn invoke(&self, route: &str, _: Value) -> Result<Value> {
        assert!(matches!(route, "friendsAck" | "friendsDeliveryStatus"));
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
