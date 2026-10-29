use super::*;

#[tokio::test]
async fn neighbors_require_a_fresh_observed_free_entitlement_and_resume_after_upgrade() {
    let mut free = paid_grant();
    free.grant = None;
    free.entitlements = Some(Entitlements::default());
    let provider = GrantReplies::new(vec![Ok(free), Err("offline".into()), Ok(paid_grant())]);
    let (_, clock) = clock(1_700_000_060);
    let billing = Billing::new(provider).with_test_grants(
        activation("fixture-online-v1"), "fixture-installation",
        Arc::new(MemoryGrants::default()), clock,
    );
    let user = session("fixture-user");
    assert!(!billing.neighbors_locked("fixture-user"));
    billing.snapshot(Some(&user));
    assert!(!billing.neighbors_locked("fixture-user"));
    let now = Instant::now();
    billing.refresh(Some(&user), RefreshEvent::SignIn, now).await;
    assert!(billing.neighbors_locked("fixture-user"));
    assert!(!billing.neighbors_locked("other-account"));
    billing.refresh(Some(&user), RefreshEvent::Explicit, now + Duration::from_secs(1)).await;
    assert!(!billing.neighbors_locked("fixture-user"));
    billing.refresh(Some(&user), RefreshEvent::Explicit, now + Duration::from_secs(2)).await;
    assert_eq!(billing.snapshot(Some(&user)).entitlements.plan, Plan::Pro);
    assert!(!billing.neighbors_locked("fixture-user"));
}
