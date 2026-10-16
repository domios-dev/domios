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
    let verified = reading(UNSIGNED);
    assert_eq!(
        verified.entitlements,
        Some(Entitlements {
            plan: Plan::Pro,
            current_period_end: 1700604800,
            ..Entitlements::default()
        })
    );
    let mut sponsor = envelope(UNSIGNED);
    sponsor["billing"]["plan"] = "sponsor".into();
    sponsor["billing"]["period_end"] = serde_json::Value::Null;
    let sponsor = Reading::from(serde_json::from_value::<ApplicationEnvelope>(sponsor).unwrap());
    assert_eq!(sponsor.entitlements.as_ref().unwrap().plan, Plan::Sponsor);
    assert_eq!(sponsor.entitlements.unwrap().current_period_end, 0);
}
const UNSIGNED: &str = include_str!("../fixtures/unsigned_schema2_response.json");
const ADMIN_GRANT: &str = include_str!("../fixtures/admin_grant_response.json");
fn envelope(fixture: &str) -> serde_json::Value {
    serde_json::from_str(fixture).unwrap()
}
fn reading(fixture: &str) -> Reading {
    Reading::from(serde_json::from_str::<ApplicationEnvelope>(fixture).unwrap())
}
fn parses(value: serde_json::Value) -> bool {
    serde_json::from_value::<ApplicationEnvelope>(value).is_ok()
}
#[test]
fn envelope_verified_pro_maps_entitlements_and_access() {
    let reading = reading(UNSIGNED);
    let entitlements = reading.entitlements.unwrap();
    assert_eq!(entitlements.plan, Plan::Pro);
    assert_eq!(entitlements.status, Status::Active);
    assert!(!entitlements.cancel_at_period_end);
    assert!(entitlement_allows(&entitlements, Capability::PaidFeatures));
    assert_eq!(
        reading.access,
        Access {
            authority: Authority::Billing,
            full_access: false,
            max_mesh_machines: None,
            max_workspaces: None,
            sponsor: false,
            priority_support: false,
        }
    );
}
#[test]
fn envelope_free_with_null_billing_maps_free() {
    let mut value = envelope(UNSIGNED);
    value["billing"] = serde_json::Value::Null;
    value["application_access"] = serde_json::json!({"schema_version":1,"authority":"free","full_access":false,"max_mesh_machines":2,"max_workspaces":2,"priority_support":false,"sponsor":false});
    let reading = Reading::from(serde_json::from_value::<ApplicationEnvelope>(value).unwrap());
    let entitlements = reading.entitlements.unwrap();
    assert_eq!(entitlements, Entitlements::default());
    assert!(!entitlement_allows(&entitlements, Capability::PaidFeatures));
    assert_eq!(reading.access.authority, Authority::Free);
    assert_eq!(reading.access.max_workspaces, Some(2));
}
#[test]
fn envelope_admin_carries_full_access() {
    let reading = reading(ADMIN_GRANT);
    assert_eq!(reading.entitlements.unwrap().plan, Plan::Free);
    assert_eq!(reading.access.authority, Authority::Admin);
    assert!(reading.access.full_access);
    assert!(reading.access.priority_support);
    assert_eq!(reading.access.max_mesh_machines, None);
}
#[test]
fn envelope_ignores_grant_and_consent_riders() {
    let mut value = envelope(ADMIN_GRANT);
    assert!(value.get("grant").is_some());
    value["consent"] = serde_json::json!({"state":"on","revision":3});
    let mut bare = value.clone();
    bare.as_object_mut().unwrap().remove("grant");
    bare.as_object_mut().unwrap().remove("consent");
    assert_eq!(
        Reading::from(serde_json::from_value::<ApplicationEnvelope>(value).unwrap()),
        Reading::from(serde_json::from_value::<ApplicationEnvelope>(bare).unwrap())
    );
}
#[test]
fn envelope_inner_records_are_exact() {
    assert!(parses(envelope(UNSIGNED)));
    for (pointer, key) in [
        ("/billing", "extra"),
        ("/billing/capabilities", "extra"),
        ("/application_access", "extra"),
    ] {
        let mut value = envelope(UNSIGNED);
        value.pointer_mut(pointer).unwrap()[key] = true.into();
        assert!(!parses(value), "{pointer} accepted an unknown field");
    }
    for (pointer, bad) in [
        ("/schema_version", serde_json::json!(3)),
        ("/billing/schema_version", serde_json::json!(2)),
        ("/application_access/schema_version", serde_json::json!(2)),
        ("/billing/plan", serde_json::json!("private")),
        ("/billing_inventory", serde_json::json!("unknown")),
        ("/application_access/authority", serde_json::json!("root")),
        ("/billing/period_end", serde_json::json!(-1)),
    ] {
        let mut value = envelope(UNSIGNED);
        *value.pointer_mut(pointer).unwrap() = bad;
        assert!(!parses(value), "{pointer} accepted an invalid value");
    }
    let mut missing = envelope(UNSIGNED);
    missing["application_access"]
        .as_object_mut()
        .unwrap()
        .remove("sponsor");
    assert!(!parses(missing));
}
#[tokio::test]
async fn requires_inventory_keeps_previous_entitlements_stale() {
    struct Replies(parking_lot::Mutex<Vec<serde_json::Value>>);
    #[async_trait]
    impl BillingProvider for Replies {
        async fn entitlements(&self, _: &codeterm_account::Session) -> Result<Reading> {
            let value = self.0.lock().remove(0);
            serde_json::from_value::<ApplicationEnvelope>(value)
                .map(Reading::from)
                .map_err(|e| e.to_string())
        }
        async fn checkout_url(&self, _: &codeterm_account::Session, _: &str) -> Result<url::Url> {
            Err("unused".into())
        }
        async fn portal_url(&self, _: &codeterm_account::Session) -> Result<url::Url> {
            Err("unused".into())
        }
    }
    let mut pending = envelope(ADMIN_GRANT);
    pending["billing"] = serde_json::Value::Null;
    pending["billing_inventory"] = "requires_inventory".into();
    let billing = Billing::new(Arc::new(Replies(parking_lot::Mutex::new(vec![
        envelope(UNSIGNED),
        pending,
    ]))));
    let session = session("owner");
    let now = Instant::now();
    let first = billing
        .refresh(Some(&session), RefreshEvent::SignIn, now)
        .await;
    assert!(!first.stale);
    let second = billing
        .refresh(Some(&session), RefreshEvent::CheckoutReturned, now)
        .await;
    assert!(second.stale);
    assert_eq!(second.entitlements, first.entitlements);
    assert_eq!(second.entitlements.plan, Plan::Pro);
    assert_eq!(second.access.unwrap().authority, Authority::Admin);
    assert!(billing.entitlement_allows(Some(&session), Capability::PaidFeatures));
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
        async fn entitlements(&self, _: &codeterm_account::Session) -> Result<Reading> {
            self.started.notify_one();
            self.release.notified().await;
            Ok(Reading::current(paid()))
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
        Reading::current(Entitlements::default())
    );
    *value.lock() = paid();
    assert_eq!(
        provider.entitlements(&session).await.unwrap(),
        Reading::current(paid())
    );
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
