use crate::jcs;
use crate::payload::{Enforcement, Mode};
use crate::policy::{Policy, PolicySpec, TrustedKey};
use crate::verify::{verify, Accepted, Binding, DetachedGrant, Rejected, VerifiedGrant};
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;
use serde_json::{json, Value};

pub const PROJECT: &str = "demo-local";
pub const EPOCH: &str = "local-epoch";
pub const UID: &str = "local-user";
pub const INST: &str = "local-installation";
pub const KEY_ID: &str = "local-test-key";
pub const T: u64 = 1_700_000_000;
pub const PERIOD_END: u64 = T + 604_800;

/// Ephemeral signer generated per test run; no private material is ever committed.
pub struct LocalSigner {
    key: SigningKey,
}

impl LocalSigner {
    pub fn new() -> Self {
        Self {
            key: SigningKey::generate(&mut OsRng),
        }
    }

    pub fn spki(&self) -> String {
        let mut der = vec![
            0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
        ];
        der.extend_from_slice(self.key.verifying_key().as_bytes());
        STANDARD.encode(der)
    }

    pub fn policy(&self) -> Policy {
        Policy::new(spec(vec![TrustedKey {
            key_id: KEY_ID.into(),
            spki_der_base64: self.spki(),
        }]))
        .expect("local policy")
    }

    pub fn sign_bytes(&self, bytes: &[u8]) -> DetachedGrant {
        DetachedGrant {
            payload: URL_SAFE_NO_PAD.encode(bytes),
            signature: URL_SAFE_NO_PAD.encode(self.key.sign(bytes).to_bytes()),
            key_id: KEY_ID.into(),
            alg: "Ed25519".into(),
        }
    }

    pub fn sign(&self, payload: &Value) -> DetachedGrant {
        self.sign_bytes(jcs::encode(payload).expect("jcs").as_bytes())
    }

    pub fn check(
        &self,
        payload: &Value,
        cached: Option<&VerifiedGrant>,
        now: u64,
    ) -> Result<Accepted, Rejected> {
        verify(&self.sign(payload), &self.policy(), &binding(), cached, now)
    }

    pub fn accept(&self, payload: &Value, now: u64) -> VerifiedGrant {
        self.check(payload, None, now).expect("accepted").grant
    }
}

pub fn spec(keys: Vec<TrustedKey>) -> PolicySpec {
    PolicySpec {
        project: PROJECT.into(),
        mode: Mode::Test,
        policy_version: 1,
        policy_epoch: EPOCH.into(),
        enforcement: Enforcement {
            enabled: false,
            rollback: false,
        },
        keys,
    }
}

pub fn binding() -> Binding<'static> {
    Binding {
        uid: UID,
        installation_id: INST,
    }
}

pub fn paid() -> Value {
    json!({
        "schema_version": 1, "iss": "domios-entitlements",
        "aud": {"project": PROJECT, "env": "test"},
        "sub": UID, "inst": INST, "revision": 10, "policy_version": 1, "policy_epoch": EPOCH,
        "issued_at": T, "not_before": T, "refresh_after": T + 3_600,
        "expires_at": T + 86_400, "grace_until": T + 86_400,
        "authority": "billing", "label": "pro",
        "enforcement": {"enabled": false, "rollback": false},
        "limits": {
            "mesh.machines": {"max": null, "scope": "account"},
            "workspaces": {"max": null, "scope": "installation"},
            "mem.items": {"max": null, "scope": "installation"}
        },
        "capabilities": [], "perks": {"sponsor": false, "priority_support": true},
        "period_end": PERIOD_END
    })
}

pub fn admin() -> Value {
    let mut value = paid();
    patch(
        &mut value,
        json!({
            "authority": "admin", "label": "admin", "period_end": null, "revision": 1
        }),
    );
    value
}

pub fn free() -> Value {
    let mut value = paid();
    patch(
        &mut value,
        json!({
            "authority": "free", "label": "free", "period_end": null, "revision": 11,
            "perks": {"sponsor": false, "priority_support": false},
            "limits": {
                "mesh.machines": {"max": 2, "scope": "account"},
                "workspaces": {"max": 2, "scope": "installation"},
                "mem.items": {"max": 1000, "scope": "installation"}
            }
        }),
    );
    value
}

pub fn patch(target: &mut Value, fields: Value) {
    let (Value::Object(target), Value::Object(fields)) = (target, fields) else {
        panic!("objects")
    };
    target.extend(fields);
}

pub fn with(base: Value, fields: Value) -> Value {
    let mut value = base;
    patch(&mut value, fields);
    value
}
