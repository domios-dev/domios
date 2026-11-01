use super::*;
use std::time::Instant;

#[tokio::test]
async fn plan_payload_keeps_unsigned_tier_separate_from_signed_authority() {
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
        assert_eq!(
            payload["entitlements"]["plan"],
            if authority == "admin" {
                "admin"
            } else {
                "free"
            }
        );
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
    assert!(serde_json::to_value(&snapshot)
        .unwrap()
        .get("signed_tier")
        .is_none());
    assert_eq!(snapshot.access.unwrap().authority, Authority::Billing);
    time.store(1_700_086_400, std::sync::atomic::Ordering::SeqCst);
    let snapshot = billing.snapshot(Some(&user));
    assert_eq!(snapshot.entitlements.plan, Plan::Sponsor);
    assert!(snapshot.checking);

    assert_eq!(snapshot.access.unwrap().authority, Authority::Free);
}

#[test]
fn signed_tiers_keep_one_serialized_plan_identity() {
    for name in ["free", "pro", "max", "sponsor", "admin", "future-tier"] {
        let label = serde_json::from_value::<codeterm_entitlements::Label>(name.into()).unwrap();
        let plan = Plan::from(&label);
        assert_eq!(plan.as_str(), name);
        let encoded = serde_json::to_value(&plan).unwrap();
        assert_eq!(encoded, name);
        assert_eq!(serde_json::from_value::<Plan>(encoded).unwrap(), plan);
    }
}

#[tokio::test]
async fn max_and_future_signed_tiers_reach_snapshot_without_a_side_identity() {
    for tier in ["max", "future-tier"] {
        let issued = 1_700_000_000;
        let (activation, reading, _) = signed_paid_revisions_at(issued, issued, tier);
        let (_, clock) = clock(issued + 60);
        let billing = Billing::new(GrantReplies::new(vec![Ok(reading)])).with_test_grants(
            activation,
            "fixture-installation",
            Arc::new(MemoryGrants::default()),
            clock,
        );
        let snapshot = billing
            .refresh(
                Some(&session("fixture-user")),
                RefreshEvent::SignIn,
                Instant::now(),
            )
            .await;
        let payload = serde_json::to_value(snapshot).unwrap();
        assert_eq!(payload["entitlements"]["plan"], tier);
        assert_eq!(payload["access"]["authority"], "billing");
        assert!(payload.get("signed_tier").is_none());
    }
}
