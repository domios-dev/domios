use crate::payload::{Enforcement, Mode};
use crate::text::{safe_integer, scalar_text, valid_id};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use ed25519_dalek::VerifyingKey;
use std::collections::HashSet;

const ED25519_SPKI_PREFIX: [u8; 12] = [
    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
];
const MAX_SPKI_BASE64: usize = 256;
// Publicly known RFC 8032 key: the producer admits it only for the fixture projects in test mode.
const RFC8032_TEST_SPKI: &str = "MCowBQYDK2VwAyEA11qYAYKxCrfVS/7TyWQHOg7hcvPapiMlrwIaaPcHURo=";
const RFC8032_TEST_PROJECTS: [&str; 2] = ["demo-domios", "demo-domios-admin"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustedKey {
    pub key_id: String,
    pub spki_der_base64: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicySpec {
    pub project: String,
    pub mode: Mode,
    pub policy_version: u64,
    pub policy_epoch: String,
    pub enforcement: Enforcement,
    pub keys: Vec<TrustedKey>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyError {
    Project,
    PolicyEpoch,
    PolicyVersion,
    KeyCount,
    KeyId,
    DuplicateKeyId,
    Spki,
    TestKeyOutsideFixture,
}

/// A policy that passed the producer's `validatePolicy`; only this type can drive verification.
#[derive(Clone, Debug)]
pub struct Policy {
    spec: PolicySpec,
    keys: Vec<(String, VerifyingKey)>,
}

impl Policy {
    pub fn new(spec: PolicySpec) -> Result<Self, PolicyError> {
        if !scalar_text(&spec.project, 128) || !valid_id(&spec.project) {
            return Err(PolicyError::Project);
        }
        if !scalar_text(&spec.policy_epoch, 128) {
            return Err(PolicyError::PolicyEpoch);
        }
        if !safe_integer(spec.policy_version) || spec.policy_version < 1 {
            return Err(PolicyError::PolicyVersion);
        }
        if spec.keys.is_empty() || spec.keys.len() > 2 {
            return Err(PolicyError::KeyCount);
        }
        let mut ids = HashSet::new();
        let mut keys = Vec::with_capacity(spec.keys.len());
        for key in &spec.keys {
            if !scalar_text(&key.key_id, 128) {
                return Err(PolicyError::KeyId);
            }
            if !ids.insert(key.key_id.as_str()) {
                return Err(PolicyError::DuplicateKeyId);
            }
            let verifying = parse_spki(&key.spki_der_base64).ok_or(PolicyError::Spki)?;
            if key.spki_der_base64 == RFC8032_TEST_SPKI
                && !(spec.mode == Mode::Test
                    && RFC8032_TEST_PROJECTS.contains(&spec.project.as_str()))
            {
                return Err(PolicyError::TestKeyOutsideFixture);
            }
            keys.push((key.key_id.clone(), verifying));
        }
        Ok(Self { spec, keys })
    }

    pub fn spec(&self) -> &PolicySpec {
        &self.spec
    }

    pub(crate) fn key(&self, key_id: &str) -> Option<&VerifyingKey> {
        self.keys
            .iter()
            .find(|(id, _)| id == key_id)
            .map(|(_, key)| key)
    }
}

/// Fixed-prefix Ed25519 SPKI: canonical standard base64 of 12 prefix bytes plus a 32-byte point.
pub fn parse_spki(encoded: &str) -> Option<VerifyingKey> {
    if encoded.len() > MAX_SPKI_BASE64 {
        return None;
    }
    let der = STANDARD.decode(encoded).ok()?;
    if STANDARD.encode(&der) != encoded || der.len() != 44 || der[..12] != ED25519_SPKI_PREFIX {
        return None;
    }
    let point: [u8; 32] = der[12..].try_into().ok()?;
    VerifyingKey::from_bytes(&point).ok()
}

pub fn is_rfc8032_test_key(spki_der_base64: &str) -> bool {
    spki_der_base64 == RFC8032_TEST_SPKI
}
