use super::*;

#[tokio::test]
async fn explicit_focus_refresh_throttled_checkout_bypasses() {
    let provider = Arc::new(adapters::fake::Fake::default());
    let billing = Billing::new(provider.clone());
    let session = session("owner");
    let now = Instant::now();
    billing
        .refresh(Some(&session), RefreshEvent::SignIn, now)
        .await;
    billing
        .refresh(
            Some(&session),
            RefreshEvent::Focus,
            now + Duration::from_secs(59),
        )
        .await;
    assert_eq!(*provider.calls.lock(), 1);
    billing
        .refresh(
            Some(&session),
            RefreshEvent::Focus,
            now + Duration::from_secs(60),
        )
        .await;
    assert_eq!(*provider.calls.lock(), 2);
    billing.browser_opened(&session);
    billing
        .refresh(
            Some(&session),
            RefreshEvent::Focus,
            now + Duration::from_secs(61),
        )
        .await;
    assert_eq!(*provider.calls.lock(), 3);
}

#[tokio::test]
async fn focus_respects_signed_freshness_and_throttle_checkout_still_bypasses() {
    let provider = GrantReplies::new(vec![Ok(paid_grant())]);
    let (now, clock) = clock(1_700_000_060);
    let billing = Billing::new(provider.clone()).with_test_grants(
        activation("fixture-online-v1"),
        "fixture-installation",
        Arc::new(MemoryGrants::default()),
        clock,
    );
    let user = session("fixture-user");
    let focus = Instant::now();
    billing
        .refresh(Some(&user), RefreshEvent::SignIn, focus)
        .await;
    billing
        .refresh(Some(&user), RefreshEvent::Focus, focus)
        .await;
    assert_eq!(provider.count(), 1);
    billing
        .refresh(
            Some(&user),
            RefreshEvent::Focus,
            focus + Duration::from_secs(120),
        )
        .await;
    assert_eq!(provider.count(), 1);
    for (offset, instant) in [(120, 1_700_003_600), (180, 1_700_086_399)] {
        now.store(instant, std::sync::atomic::Ordering::SeqCst);
        let snapshot = billing
            .refresh(
                Some(&user),
                RefreshEvent::Focus,
                focus + Duration::from_secs(offset),
            )
            .await;
        assert!(snapshot.stale);
        assert!(snapshot.grant.is_some());
        assert!(billing.entitlement_allows(Some(&user), Capability::PaidFeatures));
        billing
            .refresh(
                Some(&user),
                RefreshEvent::Focus,
                focus + Duration::from_secs(offset + 1),
            )
            .await;
    }
    assert_eq!(provider.count(), 3);
    now.store(1_700_000_060, std::sync::atomic::Ordering::SeqCst);
    billing
        .refresh(Some(&user), RefreshEvent::CheckoutReturned, focus)
        .await;
    assert_eq!(provider.count(), 4);
}
