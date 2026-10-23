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
            photo_url: None,
        },
        id_token: codeterm_account::Secret::new("fixture"),
        expires_at: None,
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
    let with_grant = Reading::from(serde_json::from_value::<ApplicationEnvelope>(value).unwrap());
    let without = Reading::from(serde_json::from_value::<ApplicationEnvelope>(bare).unwrap());
    assert!(with_grant.grant.is_some());
    assert_eq!(with_grant.entitlements, without.entitlements);
    assert_eq!(with_grant.access, without.access);
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
    assert_eq!(second.access.unwrap().authority, Authority::Free);
    assert!(!billing.entitlement_allows(Some(&session), Capability::PaidFeatures));
}
#[test]
fn entitlements_reject_unknown_plan() {
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

#[test]
fn checkout_keys_map_to_entitlement_plans_and_reject_plan_names() {
    for (key, plan) in [
        ("pro_monthly", Plan::Pro),
        ("pro_yearly", Plan::Pro),
        ("sponsor_yearly", Plan::Sponsor),
    ] {
        let parsed = CheckoutKey::parse(key).unwrap();
        assert_eq!((parsed.as_str(), parsed.plan()), (key, plan));
    }
    for unknown in ["unknown", "free", "pro", "sponsor", "sponsor_monthly", ""] {
        assert_eq!(
            CheckoutKey::parse(unknown).unwrap_err(),
            "Unknown billing plan"
        );
    }
}

#[tokio::test]
async fn checkout_url_passes_the_commercial_key_through_unchanged() {
    struct CheckoutKeys(parking_lot::Mutex<Vec<String>>);
    #[async_trait]
    impl BillingProvider for CheckoutKeys {
        async fn entitlements(&self, _: &codeterm_account::Session) -> Result<Reading> {
            Err("unused".into())
        }
        async fn checkout_url(&self, _: &codeterm_account::Session, key: &str) -> Result<url::Url> {
            self.0.lock().push(key.to_string());
            url::Url::parse("https://example.test/checkout").map_err(|error| error.to_string())
        }
        async fn portal_url(&self, _: &codeterm_account::Session) -> Result<url::Url> {
            Err("unused".into())
        }
    }

    let provider = Arc::new(CheckoutKeys(parking_lot::Mutex::new(Vec::new())));
    let billing = Billing::new(provider.clone());
    let session = session("owner");
    for key in ["pro_monthly", "pro_yearly", "sponsor_yearly"] {
        billing
            .checkout_url(&session, CheckoutKey::parse(key).unwrap())
            .await
            .unwrap();
    }
    assert_eq!(
        *provider.0.lock(),
        ["pro_monthly", "pro_yearly", "sponsor_yearly"]
    );
}

#[derive(serde::Deserialize)]
struct ProducerVector {
    id: String,
    request: serde_json::Value,
    response: Box<serde_json::value::RawValue>,
    caller_context: serde_json::Value,
}
fn producer_vectors() -> Vec<ProducerVector> {
    #[derive(serde::Deserialize)]
    struct Packet {
        vectors: Vec<ProducerVector>,
    }
    serde_json::from_str::<Packet>(include_str!(
        "../../codeterm-entitlements/vectors/grant-vectors.json"
    ))
    .unwrap()
    .vectors
}
fn activation(epoch: &'static str) -> codeterm_entitlements::Activation {
    use codeterm_entitlements::*;
    static KEYS: &[CompiledKey] = &[CompiledKey {
        key_id: "e5-rfc8032-test-only",
        spki_der_base64: "MCowBQYDK2VwAyEA11qYAYKxCrfVS/7TyWQHOg7hcvPapiMlrwIaaPcHURo=",
    }];
    Activation {
        project: "demo-domios",
        mode: Mode::Test,
        policy_version: 1,
        policy_epoch: epoch,
        enforcement: Enforcement {
            enabled: false,
            rollback: false,
        },
        keys: KEYS,
    }
}
#[derive(Default)]
struct MemoryGrants(
    parking_lot::Mutex<Option<serde_json::Value>>,
    std::sync::atomic::AtomicUsize,
    std::sync::atomic::AtomicUsize,
    parking_lot::Mutex<bool>,
    parking_lot::Mutex<Option<crate::Floors>>,
);
impl GrantStore for MemoryGrants {
    fn load(&self) -> Result<Option<serde_json::Value>> {
        Ok(self.0.lock().clone())
    }
    fn save(&self, raw: &serde_json::Value) -> Result<()> {
        self.1.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        *self.0.lock() = Some(raw.clone());
        Ok(())
    }
    fn clear(&self) -> Result<()> {
        self.2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        *self.0.lock() = None;
        Ok(())
    }
    fn load_hint(&self) -> Result<bool> {
        Ok(*self.3.lock())
    }
    fn save_hint(&self, paid: bool) -> Result<()> {
        *self.3.lock() = paid;
        Ok(())
    }
    fn load_floors(&self) -> Result<Option<crate::Floors>> {
        Ok(self.4.lock().clone())
    }
    fn save_floors(&self, floors: &crate::Floors) -> Result<()> {
        *self.4.lock() = Some(floors.clone());
        Ok(())
    }
}
struct GrantReplies {
    replies: parking_lot::Mutex<std::collections::VecDeque<Result<Reading>>>,
    calls: std::sync::atomic::AtomicUsize,
}
impl GrantReplies {
    fn new(replies: Vec<Result<Reading>>) -> Arc<Self> {
        Arc::new(Self {
            replies: parking_lot::Mutex::new(replies.into()),
            calls: std::sync::atomic::AtomicUsize::new(0),
        })
    }
    fn count(&self) -> usize {
        self.calls.load(std::sync::atomic::Ordering::SeqCst)
    }
}
#[async_trait]
impl BillingProvider for GrantReplies {
    async fn entitlements(&self, _: &codeterm_account::Session) -> Result<Reading> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.replies
            .lock()
            .pop_front()
            .unwrap_or_else(|| Err("offline".into()))
    }
    async fn checkout_url(&self, _: &codeterm_account::Session, _: &str) -> Result<url::Url> {
        Err("unused".into())
    }
    async fn portal_url(&self, _: &codeterm_account::Session) -> Result<url::Url> {
        Err("unused".into())
    }
}
fn paid_grant() -> Reading {
    let vector = producer_vectors()
        .into_iter()
        .find(|v| v.id == "valid-paid")
        .unwrap();
    Reading::from(serde_json::from_str::<ApplicationEnvelope>(vector.response.get()).unwrap())
}
fn clock(
    value: u64,
) -> (
    Arc<std::sync::atomic::AtomicU64>,
    Arc<dyn Fn() -> u64 + Send + Sync>,
) {
    let value = Arc::new(std::sync::atomic::AtomicU64::new(value));
    let cloned = value.clone();
    (
        value,
        Arc::new(move || cloned.load(std::sync::atomic::Ordering::SeqCst)),
    )
}

#[tokio::test]
async fn malformed_grant_with_paid_envelope_is_compiled_free() {
    let mut reading = paid_grant();
    reading.grant = Some(serde_json::json!({"arbitrary":"invalid"}));
    let provider = GrantReplies::new(vec![Ok(reading.clone())]);
    let store = Arc::new(MemoryGrants::default());
    let id = codeterm_account::InstallationId::parse("fixture-installation".into()).unwrap();
    let billing = Billing::new(provider)
        .with_grants(&id, store.clone())
        .unwrap();
    let snapshot = billing
        .refresh(
            Some(&session("fixture-user")),
            RefreshEvent::SignIn,
            Instant::now(),
        )
        .await;
    assert_eq!(snapshot.entitlements, reading.entitlements.unwrap());
    assert_eq!(snapshot.access.unwrap().authority, Authority::Free);
    assert!(snapshot.grant.is_none());
    assert!(store.load().unwrap().is_none());
    assert!(!billing.entitlement_allows(Some(&session("fixture-user")), Capability::PaidFeatures));
}

#[tokio::test]
async fn verdict_recomputes_half_open_windows_and_offline_access() {
    use codeterm_entitlements::TimeState;
    let user = session("fixture-user");
    let provider = GrantReplies::new(vec![Ok(paid_grant())]);
    let store = Arc::new(MemoryGrants::default());
    let (now, clock) = clock(1_700_000_060);
    let billing = Billing::new(provider).with_test_grants(
        activation("fixture-online-v1"),
        "fixture-installation",
        store,
        clock,
    );
    billing
        .refresh(Some(&user), RefreshEvent::SignIn, Instant::now())
        .await;
    for (instant, state, paid) in [
        (1_700_000_000, TimeState::Fresh, true),
        (1_700_003_599, TimeState::Fresh, true),
        (1_700_003_600, TimeState::Stale, true),
        (1_700_086_399, TimeState::Stale, true),
        (1_700_086_400, TimeState::Expired, false),
    ] {
        now.store(instant, std::sync::atomic::Ordering::SeqCst);
        let snapshot = billing.snapshot(Some(&user));
        assert_eq!(snapshot.grant.unwrap().state, state);
        assert_eq!(
            billing.entitlement_allows(Some(&user), Capability::PaidFeatures),
            paid
        );
        assert_eq!(
            snapshot.access.unwrap().max_workspaces,
            if paid { None } else { Some(2) }
        );
    }
}

#[tokio::test]
async fn focus_bypasses_throttle_while_stale_checkout_still_bypasses() {
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
    for instant in [1_700_003_600, 1_700_086_399] {
        now.store(instant, std::sync::atomic::Ordering::SeqCst);
        let snapshot = billing
            .refresh(Some(&user), RefreshEvent::Focus, focus)
            .await;
        assert!(snapshot.stale);
        assert!(snapshot.grant.is_some());
        assert!(billing.entitlement_allows(Some(&user), Capability::PaidFeatures));
    }
    assert_eq!(provider.count(), 3);
    now.store(1_700_000_060, std::sync::atomic::Ordering::SeqCst);
    billing
        .refresh(Some(&user), RefreshEvent::CheckoutReturned, focus)
        .await;
    assert_eq!(provider.count(), 4);
}

#[tokio::test]
async fn grant_reload_reverifies_and_signout_epoch_and_refusal_remove_it() {
    let user = session("fixture-user");
    let raw = paid_grant().grant.unwrap();
    let store = Arc::new(MemoryGrants::default());
    store.save(&raw).unwrap();
    let (_, injected_clock) = clock(1_700_000_060);
    let make = |epoch| {
        Billing::new(GrantReplies::new(vec![])).with_test_grants(
            activation(epoch),
            "fixture-installation",
            store.clone(),
            injected_clock.clone(),
        )
    };
    let billing = make("fixture-online-v1");
    let snapshot = billing
        .refresh(Some(&user), RefreshEvent::SignIn, Instant::now())
        .await;
    assert!(snapshot.grant.is_some());
    assert_eq!(store.load().unwrap(), Some(raw.clone()));
    billing.snapshot(None);
    assert!(store.load().unwrap().is_none());
    store.save(&raw).unwrap();
    let changed = make("new-epoch");
    assert!(changed
        .refresh(Some(&user), RefreshEvent::SignIn, Instant::now())
        .await
        .grant
        .is_none());
    assert!(store.load().unwrap().is_none());
    let mut corrupted = raw;
    corrupted["signature"] = "invalid".into();
    store.save(&corrupted).unwrap();
    let reloaded = make("fixture-online-v1");
    assert!(reloaded
        .refresh(Some(&user), RefreshEvent::SignIn, Instant::now())
        .await
        .grant
        .is_none());
    assert!(store.load().unwrap().is_none());
}

#[tokio::test]
async fn phase_b_no_grant_is_compiled_free_even_with_paid_envelope() {
    let (_, clock) = clock(1_700_000_060);
    let mut unsigned = paid_grant();
    unsigned.grant = None;
    let billing = Billing::new(GrantReplies::new(vec![Ok(unsigned)])).with_test_grants(
        activation("fixture-online-v1"),
        "fixture-installation",
        Arc::new(MemoryGrants::default()),
        clock,
    );
    let user = session("fixture-user");
    let snapshot = billing
        .refresh(Some(&user), RefreshEvent::SignIn, Instant::now())
        .await;
    assert_eq!(snapshot.access.unwrap().max_workspaces, Some(2));
    assert!(!billing.entitlement_allows(Some(&user), Capability::PaidFeatures));
}

fn fixture_server(
    response: &str,
    count: usize,
) -> (String, std::thread::JoinHandle<Vec<serde_json::Value>>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}/", listener.local_addr().unwrap());
    let response = format!("{{\"result\":{response}}}");
    let thread = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for _ in 0..count {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            let header_end = loop {
                let mut buffer = [0u8; 4096];
                let length = stream.read(&mut buffer).unwrap();
                assert!(length > 0);
                bytes.extend_from_slice(&buffer[..length]);
                if let Some(offset) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                    break offset + 4;
                }
            };
            let header = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
            assert!(header.starts_with("POST /getEntitlements HTTP/1.1"));
            assert!(header
                .to_ascii_lowercase()
                .contains("authorization: bearer fixture"));
            let length: usize = header
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|value| value.trim().parse().unwrap())
                })
                .unwrap();
            while bytes.len() < header_end + length {
                let mut buffer = [0u8; 4096];
                let length = stream.read(&mut buffer).unwrap();
                assert!(length > 0);
                bytes.extend_from_slice(&buffer[..length]);
            }
            requests.push(serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap());
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
        }
        requests
    });
    (address, thread)
}

#[tokio::test]
async fn producer_vectors_replay_exact_response_bytes_through_cloud_and_cache() {
    use codeterm_entitlements::TimeState;
    for vector in producer_vectors() {
        let count = if vector.id == "older-policy-epoch" {
            2
        } else {
            1
        };
        let (address, server) = fixture_server(vector.response.get(), count);
        let inst = vector.caller_context["installation_id"].as_str().unwrap();
        let requested_inst = vector.request["installation_id"].as_str().unwrap();
        let id = codeterm_account::InstallationId::parse(requested_inst.into()).unwrap();
        let provider = Arc::new(adapters::cloud::Cloud::fixture(&address, id));
        let (_, clock) = clock(vector.caller_context["now"].as_u64().unwrap());
        let store = Arc::new(MemoryGrants::default());
        let billing = Billing::new(provider).with_test_grants(
            activation("fixture-online-v1"),
            inst,
            store.clone(),
            clock,
        );
        let user = session(vector.request["uid"].as_str().unwrap());
        let snapshot = billing
            .refresh(Some(&user), RefreshEvent::SignIn, Instant::now())
            .await;
        match vector.id.as_str() {
            "valid-paid" => assert_eq!(snapshot.grant.as_ref().unwrap().state, TimeState::Fresh),
            "valid-admin" => assert_eq!(
                snapshot.access.as_ref().unwrap().authority,
                Authority::Admin
            ),
            "signed-free-downgrade" => {
                assert_eq!(snapshot.access.as_ref().unwrap().authority, Authority::Free)
            }
            "expired-lease" | "period-ended" => {
                assert_eq!(snapshot.grant.as_ref().unwrap().state, TimeState::Expired)
            }
            _ => assert!(snapshot.grant.is_none(), "{}", vector.id),
        }
        if snapshot.grant.is_some() {
            let expected: serde_json::Value = serde_json::from_str(vector.response.get()).unwrap();
            assert_eq!(store.load().unwrap().unwrap(), expected["grant"]);
        } else {
            assert!(store.load().unwrap().is_none());
        }
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), count);
        for request in requests {
            assert_eq!(
                request,
                serde_json::json!({"data":{"installation_id":requested_inst}})
            );
        }
    }
}

#[tokio::test]
async fn policy_refusal_refetches_once_then_stays_free() {
    let mut rejected = paid_grant();
    let vector = producer_vectors()
        .into_iter()
        .find(|v| v.id == "older-policy-epoch")
        .unwrap();
    rejected.grant =
        Reading::from(serde_json::from_str::<ApplicationEnvelope>(vector.response.get()).unwrap())
            .grant;
    let provider = GrantReplies::new(vec![Ok(rejected.clone()), Ok(rejected)]);
    let (_, clock) = clock(1_700_000_060);
    let store = Arc::new(MemoryGrants::default());
    let billing = Billing::new(provider.clone()).with_test_grants(
        activation("fixture-online-v1"),
        "fixture-installation",
        store.clone(),
        clock,
    );
    let user = session("fixture-user");
    let snapshot = billing
        .refresh(Some(&user), RefreshEvent::SignIn, Instant::now())
        .await;
    assert_eq!(provider.count(), 2);
    assert!(snapshot.grant.is_none());
    assert_eq!(snapshot.access.unwrap().authority, Authority::Free);
    assert!(store.load().unwrap().is_none());
}

#[test]
fn file_grant_store_persists_detached_bytes_only() {
    let root = std::env::temp_dir().join(format!(
        "codeterm-grant-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = FileGrantStore::new(root.clone());
    let raw = paid_grant().grant.unwrap();
    assert!(store.load().unwrap().is_none());
    store.save(&raw).unwrap();
    let path = root.join("state/account/grant.json");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(&path).unwrap()).unwrap(),
        raw
    );
    let reload = FileGrantStore::new(root.clone());
    assert_eq!(reload.load().unwrap(), Some(raw.clone()));
    let policy = codeterm_entitlements::PinnedAuthority::new(activation("fixture-online-v1"));
    let verified = crate::grant_store::authenticate(
        &reload.load().unwrap().unwrap(),
        &policy,
        "fixture-user",
        "fixture-installation",
        None,
        1_700_000_060,
    )
    .unwrap();
    assert_eq!(
        verified.verdict(1_700_086_400).state,
        codeterm_entitlements::TimeState::Expired
    );
    let changed = codeterm_entitlements::PinnedAuthority::new(activation("new-epoch"));
    assert_eq!(
        crate::grant_store::authenticate(
            &raw,
            &changed,
            "fixture-user",
            "fixture-installation",
            None,
            1_700_000_060
        ),
        Err(codeterm_entitlements::Refusal::PolicyEpoch)
    );
    reload.clear().unwrap();
    assert!(!path.exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn revision_refusal_refetches_once_and_never_reinstates_paid() {
    let vector = producer_vectors()
        .into_iter()
        .find(|v| v.id == "signed-free-downgrade")
        .unwrap();
    let free =
        Reading::from(serde_json::from_str::<ApplicationEnvelope>(vector.response.get()).unwrap());
    let provider = GrantReplies::new(vec![Ok(free), Ok(paid_grant()), Ok(paid_grant())]);
    let (_, first_clock) = clock(1_700_000_060);
    let store = Arc::new(MemoryGrants::default());
    let billing = Billing::new(provider.clone()).with_test_grants(
        activation("fixture-online-v1"),
        "fixture-installation",
        store.clone(),
        first_clock,
    );
    let user = session("fixture-user");
    billing
        .refresh(Some(&user), RefreshEvent::SignIn, Instant::now())
        .await;
    let snapshot = billing
        .refresh(Some(&user), RefreshEvent::CheckoutReturned, Instant::now())
        .await;
    assert_eq!(provider.count(), 3);
    assert_eq!(snapshot.grant.unwrap().revision, 11);
    assert!(!billing.entitlement_allows(Some(&user), Capability::PaidFeatures));
    assert_eq!(
        store.load().unwrap(),
        Some(
            serde_json::from_str::<serde_json::Value>(vector.response.get()).unwrap()["grant"]
                .clone()
        )
    );

    let (activation, newer, older) = signed_paid_revisions();
    let provider = GrantReplies::new(vec![Ok(newer.clone()), Ok(older.clone()), Ok(older)]);
    let (_, injected_clock) = clock(1_700_000_060);
    let retained = Arc::new(MemoryGrants::default());
    let billing = Billing::new(provider.clone()).with_test_grants(
        activation,
        "fixture-installation",
        retained.clone(),
        injected_clock,
    );
    billing
        .refresh(Some(&user), RefreshEvent::SignIn, Instant::now())
        .await;
    let bytes = serde_json::to_vec(&retained.load().unwrap()).unwrap();
    let snapshot = billing
        .refresh(Some(&user), RefreshEvent::CheckoutReturned, Instant::now())
        .await;
    assert_eq!(provider.count(), 3);
    assert_eq!(snapshot.grant.unwrap().revision, 11);
    assert!(billing.entitlement_allows(Some(&user), Capability::PaidFeatures));
    assert_eq!(
        serde_json::to_vec(&retained.load().unwrap()).unwrap(),
        bytes
    );
}

#[test]
fn single_grant_verification_timing() {
    let raw = paid_grant().grant.unwrap();
    let policy = codeterm_entitlements::PinnedAuthority::new(activation("fixture-online-v1"));
    let started = Instant::now();
    let verified = crate::grant_store::authenticate(
        std::hint::black_box(&raw),
        &policy,
        "fixture-user",
        "fixture-installation",
        None,
        1_700_000_060,
    )
    .unwrap();
    let elapsed = started.elapsed();
    assert_eq!(
        verified.verdict(1_700_000_060).authority,
        codeterm_entitlements::Authority::Billing
    );
    eprintln!(
        "single_grant_verify profile={} elapsed_ns={}",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        elapsed.as_nanos()
    );
    if !cfg!(debug_assertions) {
        assert!(elapsed < Duration::from_millis(1), "{elapsed:?}");
    }
}

fn signed_paid_revisions() -> (codeterm_entitlements::Activation, Reading, Reading) {
    use base64::{
        engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
        Engine,
    };
    use ed25519_dalek::{Signer, SigningKey};
    let key = SigningKey::generate(&mut rand::rngs::OsRng);
    let mut spki = vec![
        0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
    ];
    spki.extend_from_slice(key.verifying_key().as_bytes());
    let mut activation = activation("fixture-online-v1");
    activation.keys = Box::leak(
        vec![codeterm_entitlements::CompiledKey {
            key_id: "ephemeral-replay-test",
            spki_der_base64: Box::leak(STANDARD.encode(spki).into_boxed_str()),
        }]
        .into_boxed_slice(),
    );
    let original = paid_grant();
    let mut payload: serde_json::Value = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(
                original.grant.as_ref().unwrap()["payload"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap(),
    )
    .unwrap();
    let mut sign = |revision| {
        payload["revision"] = serde_json::json!(revision);
        let bytes = codeterm_entitlements::jcs::encode(&payload).unwrap();
        let mut reading = original.clone();
        reading.grant = Some(
            serde_json::json!({"alg":"Ed25519", "key_id":"ephemeral-replay-test", "payload":URL_SAFE_NO_PAD.encode(bytes.as_bytes()), "signature":URL_SAFE_NO_PAD.encode(key.sign(bytes.as_bytes()).to_bytes())}),
        );
        reading
    };
    (activation, sign(11), sign(10))
}

async fn assert_reply_retains_cached_grant(reply: Reading) {
    let store = Arc::new(MemoryGrants::default());
    let provider = GrantReplies::new(vec![Ok(paid_grant()), Ok(reply)]);
    let (_, injected_clock) = clock(1_700_000_060);
    let billing = Billing::new(provider).with_test_grants(
        activation("fixture-online-v1"),
        "fixture-installation",
        store.clone(),
        injected_clock,
    );
    let user = session("fixture-user");
    let first = billing
        .refresh(Some(&user), RefreshEvent::SignIn, Instant::now())
        .await;
    let bytes = serde_json::to_vec(&store.load().unwrap()).unwrap();
    let writes = store.1.load(std::sync::atomic::Ordering::SeqCst);
    let clears = store.2.load(std::sync::atomic::Ordering::SeqCst);
    let second = billing
        .refresh(Some(&user), RefreshEvent::CheckoutReturned, Instant::now())
        .await;
    assert_eq!(first.grant, second.grant);
    assert_eq!(store.1.load(std::sync::atomic::Ordering::SeqCst), writes);
    assert_eq!(store.2.load(std::sync::atomic::Ordering::SeqCst), clears);
    assert!(billing.entitlement_allows(Some(&user), Capability::PaidFeatures));
    assert_eq!(serde_json::to_vec(&store.load().unwrap()).unwrap(), bytes);
}

#[tokio::test]
async fn envelope_only_reply_keeps_the_cached_grant() {
    let mut reply = Reading::current(Entitlements::default());
    reply.grant = None;
    assert_reply_retains_cached_grant(reply).await;
}

#[tokio::test]
async fn refused_reply_keeps_the_cached_grant_and_store_bytes() {
    for field in ["signature", "key_id"] {
        let mut reply = paid_grant();
        reply.grant.as_mut().unwrap()[field] = "invalid".into();
        assert_reply_retains_cached_grant(reply).await;
    }
}

#[test]
fn limit_gate_binds_unless_the_pinned_activation_disables_enforcement() {
    use codeterm_entitlements::LimitKey;
    let (_, clock) = clock(1_700_000_060);
    let gated = |enabled| {
        let mut activation = activation("fixture-online-v1");
        activation.enforcement.enabled = enabled;
        Billing::new(GrantReplies::new(vec![])).with_test_grants(
            activation,
            "fixture-installation",
            Arc::new(MemoryGrants::default()),
            clock.clone(),
        )
    };
    let unpinned = Billing::new(GrantReplies::new(vec![])).limit_gate();
    assert!(unpinned.admit(LimitKey::AgentTabs, 35).is_err());
    let switched_off = gated(false).limit_gate();
    assert_eq!(switched_off.admit(LimitKey::Teams, 100), Ok(()));
    let enforced = gated(true).limit_gate();
    assert_eq!(enforced.admit(LimitKey::Teams, 5), Ok(()));
    let refusal = enforced.admit(LimitKey::AgentTabs, 35).unwrap_err();
    assert_eq!((refusal.count, refusal.max), (35, 35));
}

#[test]
fn missing_verdict_while_enforcing_is_compiled_free_and_locks() {
    let billing = Billing::new(GrantReplies::new(vec![]));
    let summary = billing.limit_gate().summary();
    assert!(summary.enforcing);
    let free = codeterm_entitlements::COMPILED_FREE_LIMITS;
    assert_eq!(summary.workspaces, free.workspaces.max);
    assert!(!billing.entitlement_allows(Some(&session("fixture-user")), Capability::PaidFeatures));
}

#[path = "checkout_theme_tests.rs"]
mod checkout_theme;
#[path = "floor_tests.rs"]
mod floor_tests;
#[path = "plan_payload_tests.rs"]
mod plan_payload;
#[path = "startup_tests.rs"]
mod startup;
