use crate::payload::{Enforcement, Mode};
use crate::policy::{Policy, PolicyError, PolicySpec, TrustedKey};
use crate::verdict::Verdict;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Verify and shadow; access keeps reading the schema-2 envelope.
    A,
    /// Access reads the signed verdict.
    B,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompiledKey {
    pub key_id: &'static str,
    pub spki_der_base64: &'static str,
}

/// Release-pinned verification inputs; changing any field is a code change shipped in a release.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Activation {
    pub phase: Phase,
    pub project: &'static str,
    pub mode: Mode,
    pub policy_version: u64,
    pub policy_epoch: &'static str,
    pub enforcement: Enforcement,
    pub keys: &'static [CompiledKey],
}

/// No reviewed production epoch or signing key is pinned yet, so nothing is verified or enforced.
pub const ACTIVATION: Option<Activation> = None;

impl Activation {
    pub fn policy(&self) -> Result<Policy, PolicyError> {
        Policy::new(PolicySpec {
            project: self.project.to_string(),
            mode: self.mode,
            policy_version: self.policy_version,
            policy_epoch: self.policy_epoch.to_string(),
            enforcement: self.enforcement,
            keys: self
                .keys
                .iter()
                .map(|key| TrustedKey {
                    key_id: key.key_id.to_string(),
                    spki_der_base64: key.spki_der_base64.to_string(),
                })
                .collect(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessSource {
    Envelope,
    Verdict,
    CompiledFree,
}

/// Phase B never falls back to the unsigned envelope: without a verdict it grants compiled free.
pub fn access_source(phase: Phase, verdict: Option<&Verdict>) -> AccessSource {
    match (phase, verdict) {
        (Phase::A, _) => AccessSource::Envelope,
        (Phase::B, Some(_)) => AccessSource::Verdict,
        (Phase::B, None) => AccessSource::CompiledFree,
    }
}
