//! Launch-binary availability parity. The verdict crosses the provider
//! boundary, so a JS drop-in and a native provider must read the same machine
//! the same way — including the unknown state, which is the one a drop-in
//! could most easily collapse into "not installed".

use super::*;

use crate::provider::ProviderAvailability;

const MISSING_BINARY: &str = "codeterm-parity-binary-that-cannot-exist";

struct NativeAvailabilityStub {
    binary_name: &'static str,
}

#[async_trait::async_trait]
impl AgentProvider for NativeAvailabilityStub {
    fn id(&self) -> &str {
        "availability-fixture"
    }

    fn display_name(&self) -> &str {
        "Availability Fixture"
    }

    fn binary_name(&self) -> &str {
        self.binary_name
    }

    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }

    fn readiness_strategy(&self) -> crate::provider::ReadinessStrategy {
        crate::provider::ReadinessStrategy::FixedDelay {
            duration: std::time::Duration::ZERO,
        }
    }

    fn task_delivery(&self) -> crate::provider::TaskDelivery {
        crate::provider::TaskDelivery::PtyInput
    }

    fn detect_from_title(&self, _: &str) -> bool {
        false
    }

    fn detect_from_output(&self, _: &str) -> bool {
        false
    }

    fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
        None
    }

    fn detect_session_id(
        &self,
        _: &str,
        _: &[String],
        _: Option<std::time::Duration>,
    ) -> Option<String> {
        None
    }
}

fn js_with_binary_name(id: &str, binary_name: Option<&str>) -> JsProvider {
    use std::sync::Arc;

    let binary = binary_name
        .map(|name| format!(r#""binaryName":"{name}","#))
        .unwrap_or_default();
    let manifest_json = format!(
        r#"{{"id":"{id}","version":"0.1.0","displayName":"Availability Fixture",{binary}
            "spawn":{{"readinessStrategy":{{"kind":"fixed_delay","durationMs":0}},
                     "taskDelivery":"pty_input","verifySubmit":{{"kind":"none"}}}}}}"#
    );
    let manifest: super::super::PluginManifest = serde_json::from_str(&manifest_json).unwrap();
    let vm = super::super::PluginVm::from_source_with(
        id,
        "module.exports.default = {};".to_string(),
        Some(manifest_json),
    )
    .expect("availability fixture vm");
    JsProvider::new(manifest, Arc::new(vm))
}

#[test]
fn declared_launch_binary_agrees_across_the_provider_boundary() {
    let native = NativeAvailabilityStub {
        binary_name: MISSING_BINARY,
    };
    let js = js_with_binary_name("availability-missing", Some(MISSING_BINARY));

    assert_eq!(js.binary_name(), native.binary_name());
    assert_eq!(
        js.observe_launch_binary().availability,
        native.observe_launch_binary().availability
    );
    assert_ne!(
        native.observe_launch_binary().availability,
        ProviderAvailability::Available,
        "a binary that cannot exist must not read as installed"
    );
}

/// Omitted JS metadata takes the trait default on both sides of the boundary.
#[test]
fn default_launch_binary_agrees_across_the_provider_boundary() {
    let native = NativeAvailabilityStub {
        binary_name: "availability-default",
    };
    let js = js_with_binary_name("availability-default", None);

    assert_eq!(js.binary_name(), native.binary_name());
    assert_eq!(
        js.observe_launch_binary().availability,
        native.observe_launch_binary().availability
    );
}

/// `describe()` is I/O-free by contract on both paths, so neither may claim an
/// observation. Enumeration is the only thing allowed to fill the field in.
#[test]
fn describe_claims_no_observation_on_either_path() {
    let native = NativeAvailabilityStub {
        binary_name: MISSING_BINARY,
    };
    let js = js_with_binary_name("availability-describe", Some(MISSING_BINARY));

    assert_eq!(
        js.describe().availability,
        ProviderAvailability::Unknown,
        "a JS drop-in must not self-report installation"
    );
    assert_eq!(js.describe().availability, native.describe().availability);
}
