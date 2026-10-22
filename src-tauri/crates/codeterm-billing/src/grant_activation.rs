use codeterm_entitlements::{Activation, ACTIVATION};

pub(crate) fn current() -> Option<Activation> {
    #[cfg(debug_assertions)]
    {
        select(
            true,
            codeterm_account::firebase_endpoints::current().emulator,
        )
    }
    #[cfg(not(debug_assertions))]
    {
        ACTIVATION
    }
}

#[cfg(any(debug_assertions, test))]
fn select(dev_build: bool, emulator_opt_in: bool) -> Option<Activation> {
    if dev_build && emulator_opt_in {
        Some(DEV_ACTIVATION)
    } else {
        ACTIVATION
    }
}

#[cfg(any(debug_assertions, test))]
const DEV_ACTIVATION: Activation = Activation {
    phase: codeterm_entitlements::Phase::A,
    project: "demo-domios",
    mode: codeterm_entitlements::Mode::Test,
    policy_version: 1,
    policy_epoch: "fixture-online-v1",
    enforcement: codeterm_entitlements::Enforcement {
        enabled: false,
        rollback: false,
    },
    keys: &[codeterm_entitlements::CompiledKey {
        key_id: "e5-rfc8032-test-only",
        spki_der_base64: "MCowBQYDK2VwAyEA11qYAYKxCrfVS/7TyWQHOg7hcvPapiMlrwIaaPcHURo=",
    }],
};

#[cfg(test)]
mod tests;
