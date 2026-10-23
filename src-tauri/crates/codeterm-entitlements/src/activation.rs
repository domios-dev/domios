use crate::payload::{Enforcement, Mode};
use crate::policy::{Policy, PolicyError, PolicySpec, TrustedKey};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompiledKey {
    pub key_id: &'static str,
    pub spki_der_base64: &'static str,
}

/// Build-pinned verification inputs; changing any field is a code change shipped in a release.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Activation {
    pub project: &'static str,
    pub mode: Mode,
    pub policy_version: u64,
    pub policy_epoch: &'static str,
    pub enforcement: Enforcement,
    pub keys: &'static [CompiledKey],
}

const ENFORCE: Enforcement = Enforcement {
    enabled: true,
    rollback: false,
};

pub const PRODUCTION: Activation = Activation {
    project: "domios-d9357",
    mode: Mode::Live,
    policy_version: 1,
    policy_epoch: "live-v1",
    enforcement: ENFORCE,
    keys: &[CompiledKey {
        key_id: "live-pending-t3",
        spki_der_base64: "MCowBQYDK2VwAyEAaQp6opvPShfljv9ougf3ikOeGcy9ZFJqkat9th3DN8k=",
    }],
};

pub const TEST: Activation = Activation {
    project: "domios-test",
    mode: Mode::Test,
    policy_version: 1,
    policy_epoch: "test-v1",
    enforcement: ENFORCE,
    keys: &[CompiledKey {
        key_id: "test-pending-t3",
        spki_der_base64: "MCowBQYDK2VwAyEAtXQE+LXeu/VsbJ48oN1SdqkSerOt+OJpvkqLjLrDXa4=",
    }],
};

#[cfg(not(debug_assertions))]
pub const ACTIVATION: Activation = PRODUCTION;
#[cfg(debug_assertions)]
pub const ACTIVATION: Activation = TEST;

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
