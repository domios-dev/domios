use super::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use codeterm_billing::{Billing, BillingProvider, Reading, RefreshEvent};
use ed25519_dalek::{Signer, SigningKey};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

#[derive(Clone)]
pub(in crate::service) struct Paid {
    pub billing: Arc<Billing>,
    pub now: Arc<AtomicU64>,
    pub provider: Arc<Provider>,
}

pub(in crate::service) struct Provider(pub parking_lot::Mutex<Result<Reading, String>>);

#[async_trait::async_trait]
impl BillingProvider for Provider {
    async fn entitlements(&self, _: &codeterm_account::Session) -> Result<Reading, String> {
        self.0.lock().clone()
    }
    async fn checkout_url(
        &self,
        _: &codeterm_account::Session,
        _: &str,
    ) -> Result<url::Url, String> {
        panic!("No checkout")
    }
    async fn portal_url(&self, _: &codeterm_account::Session) -> Result<url::Url, String> {
        panic!("No portal")
    }
}

pub(in crate::service) async fn bind(
    service: &ServiceContext,
    session: &codeterm_account::Session,
    root: &std::path::Path,
) -> Paid {
    use codeterm_entitlements::{Activation, CompiledKey, Enforcement, Mode};
    let activation = Activation {
        project: "demo-domios",
        mode: Mode::Test,
        policy_version: 1,
        policy_epoch: "fixture-online-v1",
        enforcement: Enforcement {
            enabled: false,
            rollback: false,
        },
        keys: &[CompiledKey {
            key_id: "e5-rfc8032-test-only",
            spki_der_base64: "MCowBQYDK2VwAyEA11qYAYKxCrfVS/7TyWQHOg7hcvPapiMlrwIaaPcHURo=",
        }],
    };
    let reading = reading(&session.identity.uid, "valid-paid", 10, 0);
    let provider = Arc::new(Provider(parking_lot::Mutex::new(Ok(reading))));
    let now = Arc::new(AtomicU64::new(1_700_000_060));
    let clock = now.clone();
    let billing = Arc::new(Billing::new(provider.clone()).with_test_grants(
        activation,
        "fixture-installation",
        Arc::new(codeterm_billing::FileGrantStore::new(root.to_owned())),
        Arc::new(move || clock.load(Ordering::SeqCst)),
    ));
    billing
        .refresh(
            Some(session),
            RefreshEvent::SignIn,
            std::time::Instant::now(),
        )
        .await;
    assert!(!billing.neighbors_locked(&session.identity.uid));
    assert!(service
        .ext::<crate::service::billing::BillingCell>()
        .0
        .set(billing.clone())
        .is_ok());
    let paid = Paid {
        billing,
        now,
        provider,
    };
    service.extensions.get_or_init(|| paid.clone());
    paid
}

pub(in crate::service) fn reading(uid: &str, vector: &str, revision: u64, shift: u64) -> Reading {
    let vectors: Value = serde_json::from_str(include_str!(
        "../../../../codeterm-entitlements/vectors/grant-vectors.json"
    ))
    .unwrap();
    let mut response = vectors["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == vector)
        .unwrap()["response"]
        .clone();
    let mut payload: Value = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(response["grant"]["payload"].as_str().unwrap())
            .unwrap(),
    )
    .unwrap();
    payload["sub"] = json!(uid);
    payload["revision"] = json!(revision);
    for field in [
        "issued_at",
        "not_before",
        "refresh_after",
        "expires_at",
        "grace_until",
        "period_end",
    ] {
        if let Some(value) = payload[field].as_u64() {
            payload[field] = json!(value + shift);
        }
    }
    let bytes = codeterm_entitlements::jcs::encode(&payload).unwrap();
    let seed = [
        0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c,
        0xc4, 0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae,
        0x7f, 0x60,
    ];
    response["grant"]["payload"] = json!(URL_SAFE_NO_PAD.encode(bytes.as_bytes()));
    response["grant"]["signature"] = json!(URL_SAFE_NO_PAD.encode(
        SigningKey::from_bytes(&seed)
            .sign(bytes.as_bytes())
            .to_bytes()
    ));
    Reading::from(
        serde_json::from_value::<codeterm_billing::ApplicationEnvelope>(response).unwrap(),
    )
}
