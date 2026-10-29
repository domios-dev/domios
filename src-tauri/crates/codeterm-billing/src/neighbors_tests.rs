use super::*;

#[tokio::test]
async fn neighbors_require_a_fresh_observed_free_entitlement_and_resume_after_upgrade() {
    let mut free = paid_grant();
    free.grant = None;
    free.entitlements = Some(Entitlements::default());
    free.access.authority = Authority::Free;
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

#[tokio::test]
async fn paid_expiry_missing_proof_and_not_yet_valid_proof_are_not_free_observations() {
    for (now, missing) in [(1_700_000_060, false), (1_699_000_000, false), (1_700_000_060, true)] {
        let mut paid = paid_grant();
        if missing { paid.grant = None; }
        let (time, clock) = clock(now);
        let billing = Billing::new(GrantReplies::new(vec![Ok(paid)])).with_test_grants(
            activation("fixture-online-v1"), "fixture-installation", Arc::new(MemoryGrants::default()), clock,
        );
        let user = session("fixture-user");
        billing.refresh(Some(&user), RefreshEvent::SignIn, Instant::now()).await;
        assert!(!billing.neighbors_locked("fixture-user"));
        time.store(1_700_086_400, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(billing.snapshot(Some(&user)).access.unwrap().authority, Authority::Free);
        assert!(!billing.neighbors_locked("fixture-user"));
    }
}

struct PendingPaid {
    calls: std::sync::atomic::AtomicUsize,
    started: tokio::sync::Notify,
    release: tokio::sync::Semaphore,
}
#[async_trait]
impl BillingProvider for PendingPaid {
    async fn entitlements(&self, _: &codeterm_account::Session) -> Result<Reading> {
        if self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) > 0 {
            self.started.notify_one();
            self.release.acquire().await.unwrap().forget();
        }
        Ok(paid_grant())
    }
    async fn checkout_url(&self, _: &codeterm_account::Session, _: &str) -> Result<url::Url> { Err("unused".into()) }
    async fn portal_url(&self, _: &codeterm_account::Session) -> Result<url::Url> { Err("unused".into()) }
}

#[tokio::test]
async fn paid_expiry_while_refresh_is_pending_does_not_stop_neighbors() {
    let provider = Arc::new(PendingPaid { calls: 0.into(), started: tokio::sync::Notify::new(), release: tokio::sync::Semaphore::new(0) });
    let (time, clock) = clock(1_700_000_060);
    let billing = Billing::new(provider.clone()).with_test_grants(
        activation("fixture-online-v1"), "fixture-installation", Arc::new(MemoryGrants::default()), clock,
    );
    let user = session("fixture-user");
    billing.refresh(Some(&user), RefreshEvent::SignIn, Instant::now()).await;
    let refresh = billing.refresh(Some(&user), RefreshEvent::Explicit, Instant::now() + Duration::from_secs(1));
    let control = async {
        provider.started.notified().await;
        time.store(1_700_086_400, std::sync::atomic::Ordering::SeqCst);
        assert!(!billing.neighbors_locked("fixture-user"));
        provider.release.add_permits(1);
    };
    tokio::join!(refresh, control);
}
