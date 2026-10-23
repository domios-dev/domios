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

#[tokio::test]
async fn displayed_plan_follows_verified_label_and_expiry_over_unsigned_envelope() {
    let mut reading = paid_grant();
    reading.entitlements.as_mut().unwrap().plan = Plan::Sponsor;
    let (time, clock) = clock(1_700_000_060);
    let billing = Billing::new(GrantReplies::new(vec![Ok(reading)])).with_test_grants(
        activation("fixture-online-v1"),
        "fixture-installation",
        Arc::new(MemoryGrants::default()),
        clock,
    );
    let user = session("fixture-user");
    let snapshot = billing
        .refresh(Some(&user), RefreshEvent::SignIn, Instant::now())
        .await;
    assert_eq!(snapshot.entitlements.plan, Plan::Pro);
    assert_eq!(snapshot.access.unwrap().authority, Authority::Billing);
    time.store(1_700_086_400, std::sync::atomic::Ordering::SeqCst);
    let snapshot = billing.snapshot(Some(&user));
    assert_eq!(snapshot.entitlements.plan, Plan::Free);
    assert_eq!(snapshot.access.unwrap().authority, Authority::Free);
}
