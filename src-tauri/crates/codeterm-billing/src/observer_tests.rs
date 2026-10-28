use super::*;

#[tokio::test]
async fn checkout_return_counts_once_on_focus_and_excludes_the_customer_portal() {
    let events = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let recorded = events.clone();
    let billing = Billing::new(Arc::new(adapters::fake::Fake::default()))
        .with_observer(Arc::new(move |event| recorded.lock().push(event)));
    let user = session("fixture-user");
    billing.browser_opened(&user);
    billing
        .refresh(Some(&user), RefreshEvent::Focus, Instant::now())
        .await;
    assert!(!events
        .lock()
        .iter()
        .any(|event| matches!(event, ProductEvent::CheckoutReturned)));
    billing.checkout_browser_opened(&user, Plan::Pro);
    billing
        .refresh(Some(&user), RefreshEvent::Focus, Instant::now())
        .await;
    billing
        .refresh(Some(&user), RefreshEvent::CheckoutReturned, Instant::now())
        .await;
    assert_eq!(
        events
            .lock()
            .iter()
            .filter(|event| matches!(event, ProductEvent::CheckoutReturned))
            .count(),
        1
    );
}

#[tokio::test]
async fn subscription_observer_uses_verified_plan_and_deduplicates_unsigned_changes() {
    let events = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let recorded = events.clone();
    let mut unsigned = paid_grant();
    unsigned.grant = None;
    let provider = GrantReplies::new(vec![
        Ok(Reading::current(Entitlements::default())),
        Ok(unsigned),
        Ok(paid_grant()),
        Ok(paid_grant()),
        Ok(paid_grant()),
    ]);
    let (time, clock) = clock(1_700_000_060);
    let billing = Billing::new(provider)
        .with_test_grants(
            activation("fixture-online-v1"),
            "fixture-installation",
            Arc::new(MemoryGrants::default()),
            clock,
        )
        .with_observer(Arc::new(move |event| recorded.lock().push(event)));
    let user = session("fixture-user");
    for expected in [Plan::Free, Plan::Free, Plan::Pro, Plan::Pro] {
        let snapshot = billing
            .refresh(Some(&user), RefreshEvent::CheckoutReturned, Instant::now())
            .await;
        assert_eq!(snapshot.entitlements.plan, expected);
    }
    time.store(1_700_086_400, std::sync::atomic::Ordering::SeqCst);
    let expired = billing
        .refresh(Some(&user), RefreshEvent::CheckoutReturned, Instant::now())
        .await;
    assert_eq!(expired.entitlements.plan, Plan::Free);
    assert_eq!(
        *events.lock(),
        vec![
            ProductEvent::SubscriptionChanged {
                plan: Plan::Free,
                status: Status::Active
            },
            ProductEvent::SubscriptionChanged {
                plan: Plan::Pro,
                status: Status::Active
            },
            ProductEvent::SubscriptionChanged {
                plan: Plan::Free,
                status: Status::Active
            },
        ]
    );
}
