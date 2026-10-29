use super::*;

#[tokio::test]
async fn valid_signed_grant_wins_over_unsigned_free_and_transport_failure() {
    let unsigned = Reading::current(Entitlements::default());
    let provider = GrantReplies::new(vec![
        Ok(paid_grant()),
        Ok(unsigned),
        Err("Installation proof unavailable".into()),
    ]);
    let (time, clock) = clock(1_700_000_060);
    let billing = Billing::new(provider).with_test_grants(
        activation("fixture-online-v1"),
        "fixture-installation",
        Arc::new(MemoryGrants::default()),
        clock,
    );
    let user = session("fixture-user");
    for _ in 0..3 {
        let snapshot = billing
            .refresh(Some(&user), RefreshEvent::CheckoutReturned, Instant::now())
            .await;
        assert_eq!(snapshot.entitlements.plan, Plan::Pro);
        assert!(!snapshot.checking);
        assert_eq!(billing.limit_gate().summary().workspaces, None);
    }
    time.store(1_700_086_400, std::sync::atomic::Ordering::SeqCst);
    let snapshot = billing.snapshot(Some(&user));
    assert_eq!(snapshot.entitlements.plan, Plan::Free);
    assert!(billing.limit_gate().summary().enforcing);
}

#[tokio::test]
async fn unsigned_admin_or_paid_stays_verifying_and_never_creates_workspace_locks() {
    for mut unsigned in [reading(UNSIGNED), reading(ADMIN_GRANT)] {
        unsigned.grant = None;
        let expected = unsigned.entitlements.as_ref().unwrap().plan.clone();
        let billing = Billing::new(GrantReplies::new(vec![Ok(unsigned), Err("offline".into())]));
        let user = session("fixture-user");
        for _ in 0..2 {
            let snapshot = billing
                .refresh(Some(&user), RefreshEvent::CheckoutReturned, Instant::now())
                .await;
            assert_eq!(snapshot.entitlements.plan, expected);
            assert!(snapshot.checking);
            assert!(!billing.limit_gate().summary().enforcing);
            assert!(billing
                .limit_gate()
                .admit(codeterm_entitlements::LimitKey::Workspaces, 6)
                .is_ok());
            assert!(!billing.entitlement_allows(Some(&user), Capability::PaidFeatures));
        }
    }
}

#[tokio::test]
async fn missing_answer_is_unavailable_until_actual_free_evidence_arrives() {
    let billing = Billing::new(GrantReplies::new(vec![
        Err("Installation proof unavailable".into()),
        Ok(Reading::current(Entitlements::default())),
    ]));
    let user = session("fixture-user");
    let snapshot = billing
        .refresh(Some(&user), RefreshEvent::CheckoutReturned, Instant::now())
        .await;
    assert_ne!(snapshot.entitlements.plan, Plan::Free);
    assert!(snapshot.checking);
    assert!(!billing.limit_gate().summary().enforcing);
    let snapshot = billing
        .refresh(Some(&user), RefreshEvent::CheckoutReturned, Instant::now())
        .await;
    assert_eq!(snapshot.entitlements.plan, Plan::Free);
    assert!(!snapshot.checking);
    assert!(billing
        .limit_gate()
        .admit(codeterm_entitlements::LimitKey::Workspaces, 6)
        .is_err());
}

#[tokio::test]
async fn paid_grant_expiry_does_not_invent_free_locks() {
    let (time, clock) = clock(1_700_000_060);
    let billing = Billing::new(GrantReplies::new(vec![Ok(paid_grant())])).with_test_grants(
        activation("fixture-online-v1"),
        "fixture-installation",
        Arc::new(MemoryGrants::default()),
        clock,
    );
    let user = session("fixture-user");
    billing
        .refresh(Some(&user), RefreshEvent::CheckoutReturned, Instant::now())
        .await;
    let gate = billing.limit_gate();
    time.store(1_700_086_400, std::sync::atomic::Ordering::SeqCst);
    assert!(gate
        .admit(codeterm_entitlements::LimitKey::Workspaces, 6)
        .is_ok());
    assert!(!billing.limit_gate().summary().enforcing);
    assert!(billing.snapshot(Some(&user)).checking);
    assert!(!billing.entitlement_allows(Some(&user), Capability::PaidFeatures));
}

#[test]
fn unsigned_admin_is_visible_even_when_billing_inventory_is_pending() {
    let mut value = envelope(ADMIN_GRANT);
    value["billing_inventory"] = "requires_inventory".into();
    value["billing"] = serde_json::Value::Null;
    value.as_object_mut().unwrap().remove("grant");
    let reading = Reading::from(serde_json::from_value::<ApplicationEnvelope>(value).unwrap());
    assert_eq!(reading.entitlements.unwrap().plan, Plan::Admin);
}
