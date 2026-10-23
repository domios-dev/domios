use super::*;
use std::time::Instant;

#[tokio::test]
async fn plan_payload_never_carries_unsigned_envelope_authority() {
    for (authority, full_access) in [("admin", true), ("free", false)] {
        let mut envelope: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/admin_grant_response.json")).unwrap();
        envelope["application_access"]["authority"] = authority.into();
        envelope["application_access"]["full_access"] = full_access.into();
        let reading =
            Reading::from(serde_json::from_value::<ApplicationEnvelope>(envelope).unwrap());
        let billing = Billing::new(GrantReplies::new(vec![Ok(reading)]));
        let snapshot = billing
            .refresh(
                Some(&session("owner")),
                RefreshEvent::SignIn,
                Instant::now(),
            )
            .await;
        let payload = serde_json::to_value(snapshot).unwrap();
        assert_eq!(payload["access"]["authority"], "free");
        assert_eq!(payload["access"]["full_access"], false);
        assert_eq!(payload["entitlements"]["plan"], "free");
        assert_eq!(payload["stale"], false);
        assert!(payload.get("grant").is_none());
    }
}
