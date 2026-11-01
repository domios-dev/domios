use super::*;
use async_trait::async_trait;
use std::sync::atomic::AtomicUsize;
use tokio::sync::{Notify, Semaphore};

struct Provider {
    entitlements: AtomicUsize,
    catalog: AtomicUsize,
    subscription: AtomicUsize,
    started: Notify,
    release: Semaphore,
    failure: Mutex<bool>,
    mutation_gate: Mutex<Option<Arc<Semaphore>>>,
    read_gate: Mutex<Option<Arc<Semaphore>>>,
}

impl Provider {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            entitlements: 0.into(),
            catalog: 0.into(),
            subscription: 0.into(),
            started: Notify::new(),
            release: Semaphore::new(0),
            failure: Mutex::new(false),
            mutation_gate: Mutex::new(None),
            read_gate: Mutex::new(None),
        })
    }
}

#[async_trait]
impl BillingProvider for Provider {
    async fn entitlements(&self, _: &codeterm_account::Session) -> Result<Reading> {
        self.entitlements.fetch_add(1, Ordering::SeqCst);
        self.started.notify_one();
        self.release.acquire().await.unwrap().forget();
        if *self.failure.lock() {
            Err("Billing server unreachable".into())
        } else {
            Ok(Reading::current(Entitlements::default()))
        }
    }

    async fn plan_operation(
        &self,
        _: &codeterm_account::Session,
        operation: PlanOperation,
    ) -> Result<serde_json::Value> {
        match operation {
            PlanOperation::Catalog {} => {
                self.catalog.fetch_add(1, Ordering::SeqCst);
            }
            PlanOperation::Subscription {} => {
                self.subscription.fetch_add(1, Ordering::SeqCst);
            }
            PlanOperation::Change { .. } => {
                let gate = self.mutation_gate.lock().clone();
                if let Some(gate) = gate {
                    self.started.notify_one();
                    gate.acquire().await.unwrap().forget();
                }
            }
            _ => panic!("Refresh intent must remain in the daemon"),
        }
        let gate = self.read_gate.lock().clone();
        if let Some(gate) = gate {
            self.started.notify_one();
            gate.acquire().await.unwrap().forget();
        }
        if *self.failure.lock() {
            Err("Billing server unreachable".into())
        } else {
            Ok(serde_json::json!({"value": self.subscription.load(Ordering::SeqCst)}))
        }
    }

    async fn checkout_url(&self, _: &codeterm_account::Session, _: &str) -> Result<url::Url> {
        Err("unused".into())
    }

    async fn portal_url(&self, _: &codeterm_account::Session) -> Result<url::Url> {
        Err("unused".into())
    }
}

fn fixture() -> (
    Billing,
    Arc<Provider>,
    Arc<AtomicU64>,
    codeterm_account::Session,
) {
    let provider = Provider::new();
    let time = Arc::new(AtomicU64::new(100));
    let clock = time.clone();
    let mut billing = Billing::new(provider.clone());
    billing.clock = Arc::new(move || clock.load(Ordering::SeqCst));
    let session = codeterm_account::Session {
        login_generation: 1,
        identity: codeterm_account::Identity {
            uid: "owner".into(),
            email: None,
            display_name: None,
            provider: "fake".into(),
            photo_url: None,
        },
        id_token: codeterm_account::Secret::new("fixture"),
        expires_at: None,
    };
    (billing, provider, time, session)
}

#[tokio::test]
async fn concurrent_signin_deadline_explicit_share_one_refresh() {
    let (billing, provider, _, session) = fixture();
    let mut flight = billing.admit_test_refresh(&session, RefreshEvent::SignIn);
    provider.started.notified().await;
    let deadline = billing.admit_test_refresh(&session, RefreshEvent::Deadline);
    let explicit = billing.admit_test_refresh(&session, RefreshEvent::Explicit);
    provider.release.add_permits(1);
    flight.changed().await.unwrap();
    assert!(deadline.borrow().is_some());
    assert!(explicit.borrow().is_some());
    assert_eq!(provider.entitlements.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn forced_change_during_flight_queues_one_authoritative_refresh() {
    let (billing, provider, _, session) = fixture();
    let mut flight = billing.admit_test_refresh(&session, RefreshEvent::SignIn);
    provider.started.notified().await;
    billing.admit_test_refresh(&session, RefreshEvent::PlanChanged);
    billing.admit_test_refresh(&session, RefreshEvent::PlanChanged);
    provider.release.add_permits(1);
    provider.started.notified().await;
    assert_eq!(provider.entitlements.load(Ordering::SeqCst), 2);
    assert!(flight.borrow().is_none());
    provider.release.add_permits(1);
    flight.changed().await.unwrap();
    assert_eq!(provider.entitlements.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn error_cooldown_is_shared_by_clones_reads_and_refresh_events() {
    let (billing, provider, time, session) = fixture();
    *provider.failure.lock() = true;
    provider.release.add_permits(1);
    billing
        .refresh(Some(&session), RefreshEvent::SignIn, Instant::now())
        .await;
    let other = billing.clone();
    for event in [
        RefreshEvent::SignIn,
        RefreshEvent::Deadline,
        RefreshEvent::Explicit,
    ] {
        other.refresh(Some(&session), event, Instant::now()).await;
    }
    assert!(other
        .plan_operation(&session, PlanOperation::Catalog {})
        .await
        .is_err());
    assert_eq!(provider.entitlements.load(Ordering::SeqCst), 1);
    assert_eq!(provider.catalog.load(Ordering::SeqCst), 0);
    time.store(160, Ordering::SeqCst);
    provider.release.add_permits(1);
    other
        .refresh(Some(&session), RefreshEvent::Explicit, Instant::now())
        .await;
    assert_eq!(provider.entitlements.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn catalog_and_subscription_are_session_cached_with_bounded_explicit_refresh() {
    let (billing, provider, time, mut session) = fixture();
    for operation in [PlanOperation::Catalog {}, PlanOperation::Subscription {}] {
        billing.plan_operation(&session, operation).await.unwrap();
    }
    time.store(4000, Ordering::SeqCst);
    billing
        .refresh(Some(&session), RefreshEvent::Focus, Instant::now())
        .await;
    billing
        .plan_operation(&session, PlanOperation::Catalog {})
        .await
        .unwrap();
    billing
        .plan_operation(&session, PlanOperation::Subscription {})
        .await
        .unwrap();
    assert_eq!(provider.catalog.load(Ordering::SeqCst), 1);
    assert_eq!(provider.subscription.load(Ordering::SeqCst), 1);
    assert_eq!(provider.entitlements.load(Ordering::SeqCst), 0);
    for _ in 0..2 {
        billing
            .plan_operation(&session, PlanOperation::RefreshCatalog {})
            .await
            .unwrap();
        billing
            .plan_operation(&session, PlanOperation::RefreshSubscription {})
            .await
            .unwrap();
    }
    assert_eq!(provider.catalog.load(Ordering::SeqCst), 2);
    assert_eq!(provider.subscription.load(Ordering::SeqCst), 2);
    session.login_generation += 1;
    billing
        .plan_operation(&session, PlanOperation::Catalog {})
        .await
        .unwrap();
    billing
        .plan_operation(&session, PlanOperation::Subscription {})
        .await
        .unwrap();
    assert_eq!(provider.catalog.load(Ordering::SeqCst), 3);
    assert_eq!(provider.subscription.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn plan_change_invalidates_subscription_and_refreshes_entitlements_immediately() {
    let (billing, provider, _, session) = fixture();
    billing
        .plan_operation(&session, PlanOperation::Subscription {})
        .await
        .unwrap();
    provider.release.add_permits(2);
    billing
        .refresh(Some(&session), RefreshEvent::SignIn, Instant::now())
        .await;
    billing
        .plan_operation(
            &session,
            PlanOperation::Change {
                price_id: "fixture-price".into(),
            },
        )
        .await
        .unwrap();
    billing
        .plan_operation(&session, PlanOperation::Subscription {})
        .await
        .unwrap();
    assert_eq!(provider.entitlements.load(Ordering::SeqCst), 2);
    assert_eq!(provider.subscription.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn late_plan_change_cannot_restore_a_signed_out_session() {
    let (billing, provider, _, session) = fixture();
    let gate = Arc::new(Semaphore::new(0));
    *provider.mutation_gate.lock() = Some(gate.clone());
    let change = billing.plan_operation(
        &session,
        PlanOperation::Change {
            price_id: "fixture-price".into(),
        },
    );
    let logout = async {
        provider.started.notified().await;
        billing.snapshot(None);
        gate.add_permits(1);
    };
    let (result, ()) = tokio::join!(change, logout);
    assert!(result.is_err());
    assert_eq!(provider.entitlements.load(Ordering::SeqCst), 0);
    assert_eq!(billing.snapshot(None), Snapshot::default());
}

#[tokio::test]
async fn delayed_old_admissions_cannot_restore_retired_or_newer_login_generations() {
    let (billing, provider, _, old_session) = fixture();
    billing.snapshot(Some(&old_session));
    billing.snapshot(None);
    billing
        .refresh(Some(&old_session), RefreshEvent::SignIn, Instant::now())
        .await;
    assert!(billing
        .plan_operation(&old_session, PlanOperation::Catalog {})
        .await
        .is_err());
    assert_eq!(provider.entitlements.load(Ordering::SeqCst), 0);
    assert_eq!(provider.catalog.load(Ordering::SeqCst), 0);
    let mut current = old_session.clone();
    current.login_generation += 1;
    billing
        .plan_operation(&current, PlanOperation::Catalog {})
        .await
        .unwrap();
    assert_eq!(billing.snapshot(Some(&old_session)), Snapshot::default());
    billing
        .refresh(
            Some(&old_session),
            RefreshEvent::PlanChanged,
            Instant::now(),
        )
        .await;
    assert!(billing
        .plan_operation(&old_session, PlanOperation::Subscription {})
        .await
        .is_err());
    billing
        .plan_operation(&current, PlanOperation::Catalog {})
        .await
        .unwrap();
    assert_eq!(provider.catalog.load(Ordering::SeqCst), 1);
    assert_eq!(provider.entitlements.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn second_refresh_defers_further_invalidations_and_finishes_waiters() {
    let (billing, provider, time, session) = fixture();
    let mut flight = billing.admit_test_refresh(&session, RefreshEvent::SignIn);
    provider.started.notified().await;
    billing.admit_test_refresh(&session, RefreshEvent::PlanChanged);
    provider.release.add_permits(1);
    provider.started.notified().await;
    billing.admit_test_refresh(&session, RefreshEvent::PlanChanged);
    provider.release.add_permits(1);
    flight.changed().await.unwrap();
    assert_eq!(provider.entitlements.load(Ordering::SeqCst), 2);
    assert!(flight.borrow().as_ref().unwrap().stale);
    assert!(billing.cache.lock().refresh_queued);
    time.store(160, Ordering::SeqCst);
    provider.release.add_permits(1);
    billing
        .refresh(Some(&session), RefreshEvent::Explicit, Instant::now())
        .await;
    assert_eq!(provider.entitlements.load(Ordering::SeqCst), 3);
    assert!(!billing.cache.lock().refresh_queued);
}

#[tokio::test]
async fn browser_return_seam_refreshes_once_and_rejects_stale_accounts() {
    let (billing, provider, _, session) = fixture();
    let events = Arc::new(Mutex::new(Vec::new()));
    let recorded = events.clone();
    let billing = billing.with_observer(Arc::new(move |event| recorded.lock().push(event)));
    let focus = billing.browser_return_event(&session, RefreshEvent::Focus);
    billing.refresh(Some(&session), focus, Instant::now()).await;
    assert_eq!(provider.entitlements.load(Ordering::SeqCst), 0);
    billing.checkout_browser_opened(&session, Plan::Pro);
    let mut foreign = session.clone();
    foreign.identity.uid = "foreign".into();
    assert!(billing.browser_return_event(&foreign, RefreshEvent::Focus) == RefreshEvent::Focus);
    foreign = session.clone();
    foreign.login_generation += 1;
    assert!(billing.browser_return_event(&foreign, RefreshEvent::Focus) == RefreshEvent::Focus);
    provider.release.add_permits(1);
    let returned = billing.browser_return_event(&session, RefreshEvent::Focus);
    assert!(returned == RefreshEvent::CheckoutReturned);
    billing
        .refresh(Some(&session), returned, Instant::now())
        .await;
    let focus = billing.browser_return_event(&session, RefreshEvent::Focus);
    assert!(focus == RefreshEvent::Focus);
    billing.refresh(Some(&session), focus, Instant::now()).await;
    assert_eq!(provider.entitlements.load(Ordering::SeqCst), 1);
    billing.browser_opened(&session);
    provider.release.add_permits(1);
    let returned = billing.browser_return_event(&session, RefreshEvent::Focus);
    billing
        .refresh(Some(&session), returned, Instant::now())
        .await;
    assert_eq!(provider.entitlements.load(Ordering::SeqCst), 2);
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
async fn successful_forced_recovery_reopens_subscription_reads() {
    let (billing, provider, _, session) = fixture();
    let gate = Arc::new(Semaphore::new(0));
    *provider.read_gate.lock() = Some(gate.clone());
    *provider.failure.lock() = true;
    let mut read = billing.admit_test_read(session.clone());
    provider.started.notified().await;
    billing.cache.lock().reads[1].invalidate();
    gate.add_permits(1);
    read.changed().await.unwrap();
    assert!(read.borrow().as_ref().unwrap().is_err());
    *provider.failure.lock() = false;
    provider.release.add_permits(1);
    billing
        .refresh(Some(&session), RefreshEvent::PlanChanged, Instant::now())
        .await;
    gate.add_permits(1);
    billing
        .plan_operation(&session, PlanOperation::Subscription {})
        .await
        .unwrap();
    assert_eq!(provider.subscription.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn concurrent_reads_share_one_flight_and_replace_an_invalidated_reply_once() {
    let (billing, provider, _, session) = fixture();
    let gate = Arc::new(Semaphore::new(0));
    *provider.read_gate.lock() = Some(gate.clone());
    let first = billing.admit_test_read(session.clone());
    let mut second = billing.admit_test_read(session.clone());
    provider.started.notified().await;
    billing.cache.lock().reads[1].invalidate();
    gate.add_permits(1);
    provider.started.notified().await;
    billing.cache.lock().reads[1].invalidate();
    gate.add_permits(1);
    second.changed().await.unwrap();
    assert!(first.borrow().as_ref().unwrap().is_err());
    assert_eq!(provider.subscription.load(Ordering::SeqCst), 2);
    assert!(billing
        .plan_operation(&session, PlanOperation::Subscription {})
        .await
        .is_err());
    assert_eq!(provider.subscription.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn invalidated_read_error_obeys_shared_cooldown_without_replay() {
    let (billing, provider, time, session) = fixture();
    let gate = Arc::new(Semaphore::new(0));
    *provider.read_gate.lock() = Some(gate.clone());
    *provider.failure.lock() = true;
    let mut flight = billing.admit_test_read(session.clone());
    provider.started.notified().await;
    billing.cache.lock().reads[1].invalidate();
    billing.cache.lock().reads[1].invalidate();
    gate.add_permits(1);
    flight.changed().await.unwrap();
    assert!(flight.borrow().as_ref().unwrap().is_err());
    assert!(billing
        .plan_operation(&session, PlanOperation::Subscription {})
        .await
        .is_err());
    assert_eq!(provider.subscription.load(Ordering::SeqCst), 1);
    time.store(160, Ordering::SeqCst);
    *provider.failure.lock() = false;
    gate.add_permits(1);
    billing
        .plan_operation(&session, PlanOperation::Subscription {})
        .await
        .unwrap();
    assert_eq!(provider.subscription.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn successful_refresh_does_not_clear_a_newer_concurrent_failure() {
    let (billing, provider, _, session) = fixture();
    let mut flight = billing.admit_test_refresh(&session, RefreshEvent::SignIn);
    provider.started.notified().await;
    *provider.failure.lock() = true;
    assert!(billing
        .plan_operation(&session, PlanOperation::Catalog {})
        .await
        .is_err());
    *provider.failure.lock() = false;
    provider.release.add_permits(1);
    flight.changed().await.unwrap();
    assert!(billing
        .plan_operation(&session, PlanOperation::Subscription {})
        .await
        .is_err());
    assert_eq!(provider.subscription.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn delayed_none_snapshot_cannot_retire_a_newly_bound_login() {
    let (billing, provider, _, mut session) = fixture();
    billing.snapshot(Some(&session));
    let before_signin = billing.binding_generation();
    session.login_generation += 1;
    billing.snapshot(Some(&session));
    billing.snapshot_if_current(None, before_signin);
    billing
        .plan_operation(&session, PlanOperation::Catalog {})
        .await
        .unwrap();
    assert_eq!(provider.catalog.load(Ordering::SeqCst), 1);
    assert_eq!(
        billing.cache.lock().login_generation,
        Some(session.login_generation)
    );
}
