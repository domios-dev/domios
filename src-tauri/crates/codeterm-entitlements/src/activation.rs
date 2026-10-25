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
    policy_version: 2,
    policy_epoch: "live-v1",
    enforcement: ENFORCE,
    keys: &[CompiledKey {
        key_id: "live-2026-10",
        spki_der_base64: "MCowBQYDK2VwAyEAxixLxfHaTjofOt+fUkmr9B1hqUHUMwdTvsm7xFPxMvY=",
    }],
};

/// Injected from the owner's secret store by build.sh; unset builds DEV with no QA backend (plain free).
#[cfg(debug_assertions)]
pub const QA_PROJECT: &str = match option_env!("DOMIOS_QA_PROJECT_ID") {
    Some(project) => project,
    None => "",
};
#[cfg(not(debug_assertions))]
pub const QA_PROJECT: &str = "";

pub const TEST: Activation = Activation {
    project: QA_PROJECT,
    mode: Mode::Test,
    policy_version: 2,
    policy_epoch: "test-v1",
    enforcement: ENFORCE,
    keys: &[CompiledKey {
        key_id: "test-2026-10",
        spki_der_base64: "MCowBQYDK2VwAyEA2nQy+tOBmjHHrGnun9ZlMCKQ9fdd7HlkpF5NrykMl7A=",
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
