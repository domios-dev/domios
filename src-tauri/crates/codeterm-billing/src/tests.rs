use super::*;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
fn session(uid: &str) -> codeterm_account::Session {
    codeterm_account::Session {
        identity: codeterm_account::Identity {
            uid: uid.into(),
            email: None,
            display_name: None,
            provider: "fake".into(),
        },
        id_token: codeterm_account::Secret::new("fixture"),
    }
}
fn paid() -> Entitlements {
    Entitlements {
        plan: Plan::Pro,
        current_period_end: 2000000000,
        ..Entitlements::default()
    }
}
#[test]
fn entitlement_parsing() {
    for (value, status) in [
        ("active", Status::Active),
        ("past_due", Status::PastDue),
        ("canceled", Status::Canceled),
        ("revoked", Status::Revoked),
    ] {
        let parsed: Entitlements = serde_json::from_value(serde_json::json!({"plan":"pro","status":value,"current_period_end":2000000000,"cancel_at_period_end":true})).unwrap();
        assert_eq!(parsed.status, status);
        assert!(parsed.cancel_at_period_end);
    }
    assert!(serde_json::from_value::<Entitlements>(
        serde_json::json!({"plan":"pro","status":"unknown"})
    )
    .is_err());
}
#[test]
fn capability_gate_table() {
    assert!(entitlement_allows(
        &Entitlements::default(),
        Capability::Terminal
    ));
    assert!(!entitlement_allows(
        &Entitlements::default(),
        Capability::PaidFeatures
    ));
    assert!(entitlement_allows(&paid(), Capability::PaidFeatures));
    for status in [
        Status::PastDue,
        Status::Canceled,
        Status::Revoked,
        Status::Incomplete,
        Status::Unpaid,
        Status::Paused,
    ] {
        assert!(!entitlement_allows(
            &Entitlements { status, ..paid() },
            Capability::PaidFeatures
        ));
    }
    assert!(Plan::parse("unknown").is_err());
    assert!(serde_json::from_value::<Entitlements>(serde_json::json!({"plan":"private","status":"active","current_period_end":0,"cancel_at_period_end":false})).is_err());
}
#[tokio::test]
async fn stale_cache_transport_failure() {
    let provider = Arc::new(adapters::fake::Fake::default());
    *provider.value.lock() = Ok(paid());
    let billing = Billing::new(provider.clone());
    let session = session("owner");
    let now = Instant::now();
    assert!(
        !billing
            .refresh(Some(&session), RefreshEvent::SignIn, now)
            .await
            .stale
    );
    *provider.value.lock() = Err("offline".into());
    let stale = billing
        .refresh(Some(&session), RefreshEvent::CheckoutReturned, now)
        .await;
    assert!(stale.stale);
    assert_eq!(stale.entitlements, paid());
    let other = session_for_other();
    let failed = billing
        .refresh(Some(&other), RefreshEvent::SignIn, now)
        .await;
    assert!(failed.stale);
    assert_eq!(failed.entitlements, Entitlements::default());
}
fn session_for_other() -> codeterm_account::Session {
    session("other")
}
#[tokio::test]
async fn signed_out_returns_free() {
    let provider = Arc::new(adapters::fake::Fake::default());
    *provider.value.lock() = Ok(paid());
    let billing = Billing::new(provider.clone());
    billing
        .refresh(
            Some(&session("owner")),
            RefreshEvent::SignIn,
            Instant::now(),
        )
        .await;
    assert_eq!(
        billing
            .refresh(None, RefreshEvent::Focus, Instant::now())
            .await,
        Snapshot::default()
    );
    assert_eq!(*provider.calls.lock(), 1);
}
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
async fn late_fetch_cannot_restore_signed_out_entitlement() {
    struct Delayed {
        started: tokio::sync::Notify,
        release: tokio::sync::Notify,
    }
    #[async_trait]
    impl BillingProvider for Delayed {
        async fn entitlements(&self, _: &codeterm_account::Session) -> Result<Entitlements> {
            self.started.notify_one();
            self.release.notified().await;
            Ok(paid())
        }
        async fn checkout_url(&self, _: &codeterm_account::Session, _: &str) -> Result<url::Url> {
            Err("unused".into())
        }
        async fn portal_url(&self, _: &codeterm_account::Session) -> Result<url::Url> {
            Err("unused".into())
        }
    }
    let provider = Arc::new(Delayed {
        started: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
    });
    let billing = Arc::new(Billing::new(provider.clone()));
    let task_billing = billing.clone();
    let task = tokio::spawn(async move {
        task_billing
            .refresh(
                Some(&session("owner")),
                RefreshEvent::SignIn,
                Instant::now(),
            )
            .await
    });
    provider.started.notified().await;
    billing.snapshot(None);
    provider.release.notify_one();
    assert_eq!(task.await.unwrap(), Snapshot::default());
    assert_eq!(billing.snapshot(None), Snapshot::default());
}

#[tokio::test]
async fn local_config_and_browser_routes() {
    let value = Arc::new(parking_lot::Mutex::new(Entitlements::default()));
    let read_value = value.clone();
    let provider = adapters::local::Local {
        read: Box::new(move || Ok(read_value.lock().clone())),
        origin: url::Url::parse("http://127.0.0.1:15100/").unwrap(),
    };
    let session = session("owner");
    assert_eq!(
        provider.entitlements(&session).await.unwrap(),
        Entitlements::default()
    );
    *value.lock() = paid();
    assert_eq!(provider.entitlements(&session).await.unwrap(), paid());
    assert_eq!(
        provider
            .checkout_url(&session, "pro")
            .await
            .unwrap()
            .as_str(),
        "http://127.0.0.1:15100/billing/checkout?plan=pro"
    );
    assert_eq!(
        provider.portal_url(&session).await.unwrap().path(),
        "/billing/portal"
    );
}
