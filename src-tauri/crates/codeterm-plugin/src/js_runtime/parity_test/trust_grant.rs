//! `integration.trust_grant` capability parity (DN-1): a JS drop-in declaring
//! the capability is invoked exactly once per integration install with the
//! declared managed key(s); providers declaring nothing are unaffected on both
//! the native and JS runtimes. The grant itself is provider-owned — core only
//! sees the capability and an opaque JSON outcome.

use std::sync::Arc;
use std::sync::Mutex;

use crate::provider::{
    install_integration_with_trust_grant, AgentLaunchParams, AgentProvider,
    IntegrationInstallOptions, IntegrationTrustGrantCapability,
};

use super::{load_js, require_plugins, JsProvider};

fn install_options() -> IntegrationInstallOptions {
    IntegrationInstallOptions {
        consent: true,
        precompact_memory_export: false,
    }
}

fn trust_grant_js_fixture(id: &str, manifest_json: &str, plugin_js: &str) -> JsProvider {
    let manifest: crate::js_runtime::PluginManifest = serde_json::from_str(manifest_json).unwrap();
    let vm = crate::js_runtime::PluginVm::from_source_with(
        id,
        plugin_js.to_string(),
        Some(manifest_json.to_string()),
    )
    .expect("fixture vm");
    JsProvider::new(manifest, Arc::new(vm))
}

#[derive(Default)]
struct TrustGrantNativeStub {
    granted_calls: Mutex<Vec<Vec<String>>>,
}

#[async_trait::async_trait]
impl AgentProvider for TrustGrantNativeStub {
    fn id(&self) -> &str {
        "trust-grant-native-fixture"
    }
    fn display_name(&self) -> &str {
        "Trust Grant Native Fixture"
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
    fn session_file_path(&self, _: &str, _: &str) -> Option<std::path::PathBuf> {
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
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
    fn integration_trust_grant_capability(&self) -> Option<IntegrationTrustGrantCapability> {
        Some(IntegrationTrustGrantCapability {
            managed_keys: vec!["session_start".into()],
        })
    }
    fn grant_integration_trust(&self, managed_keys: &[String]) -> serde_json::Value {
        let mut calls = self.granted_calls.lock().unwrap();
        calls.push(managed_keys.to_vec());
        serde_json::json!({ "granted": true, "calls": calls.len(), "keys": managed_keys })
    }
}

const DECLARING_MANIFEST: &str = r#"{
    "id": "trust-grant-js-fixture",
    "version": "0.1.0",
    "displayName": "Trust Grant Fixture",
    "spawn": {
        "readinessStrategy": { "kind": "fixed_delay", "durationMs": 0 },
        "taskDelivery": "pty_input",
        "verifySubmit": { "kind": "none" }
    },
    "integration": {
        "kind": "provider_managed",
        "settingsPath": "~/.trust-grant-fixture/hooks.json",
        "trust_grant": { "managedKeys": ["session_start"] }
    }
}"#;

const DECLARING_PLUGIN_JS: &str = r#"
    let grantCalls = [];
    module.exports.default = {
        installIntegration() { return { ok: true }; },
        grantIntegrationTrust(keys) {
            grantCalls.push(keys);
            return { granted: true, calls: grantCalls.length, keys: keys };
        }
    };
"#;

#[test]
fn integration_trust_grant_invoked_once_per_install_with_managed_keys() {
    let js = trust_grant_js_fixture(
        "trust-grant-js-fixture",
        DECLARING_MANIFEST,
        DECLARING_PLUGIN_JS,
    );
    let native = TrustGrantNativeStub::default();
    let expected_keys = serde_json::json!(["session_start"]);

    let js_grant = install_integration_with_trust_grant(&js, 7686, &install_options())
        .expect("install must succeed")
        .expect("declared capability must be invoked");
    assert_eq!(
        js_grant["calls"], 1,
        "JS grant must run exactly once per install"
    );
    assert_eq!(js_grant["keys"], expected_keys);

    let native_grant = install_integration_with_trust_grant(&native, 7686, &install_options())
        .expect("install must succeed")
        .expect("declared capability must be invoked");
    assert_eq!(
        native_grant["calls"], 1,
        "native grant must run exactly once per install"
    );
    assert_eq!(native_grant["keys"], expected_keys);
    assert_eq!(
        native.granted_calls.lock().unwrap().as_slice(),
        &[vec!["session_start".to_string()]]
    );

    // Per-install, not per-provider-lifetime: a second install grants again, once.
    let js_second = install_integration_with_trust_grant(&js, 7686, &install_options())
        .expect("install must succeed")
        .expect("declared capability must be invoked");
    assert_eq!(js_second["calls"], 2);
    let native_second = install_integration_with_trust_grant(&native, 7686, &install_options())
        .expect("install must succeed")
        .expect("declared capability must be invoked");
    assert_eq!(native_second["calls"], 2);
}

#[test]
fn integration_trust_grant_absent_capability_leaves_providers_unaffected() {
    let sentinel = std::env::temp_dir().join(format!(
        "trust_grant_never_invoked_{}",
        uuid::Uuid::new_v4().simple()
    ));
    let manifest_json = r#"{
        "id": "trust-grant-undeclared-fixture",
        "version": "0.1.0",
        "displayName": "Undeclared Fixture",
        "spawn": {
            "readinessStrategy": { "kind": "fixed_delay", "durationMs": 0 },
            "taskDelivery": "pty_input",
            "verifySubmit": { "kind": "none" }
        },
        "integration": {
            "kind": "provider_managed",
            "settingsPath": "~/.trust-grant-fixture/hooks.json"
        }
    }"#;
    let plugin_js = format!(
        r#"
        module.exports.default = {{
            installIntegration() {{ return {{ ok: true }}; }},
            grantIntegrationTrust() {{
                host.fs.writeFile({sentinel:?}, "invoked");
                return {{ granted: true }};
            }}
        }};
    "#,
        sentinel = sentinel.to_string_lossy()
    );
    let js = trust_grant_js_fixture("trust-grant-undeclared-fixture", manifest_json, &plugin_js);

    let js_outcome = install_integration_with_trust_grant(&js, 7686, &install_options())
        .expect("install must succeed");
    assert!(
        js_outcome.is_none(),
        "undeclared JS capability must not be invoked"
    );
    assert!(
        !sentinel.exists(),
        "grantIntegrationTrust must never run without a manifest declaration"
    );

    // Native control: a provider overriding nothing reports no capability and
    // the same install seam completes without a grant step.
    let native = super::DiscoverModelsNativeStub;
    assert!(native.integration_trust_grant_capability().is_none());
    assert_eq!(native.describe().integration_trust_grant, None);
    assert_eq!(js.describe().integration_trust_grant, None);
}

#[test]
fn integration_trust_grant_descriptor_declares_managed_keys() {
    let js = trust_grant_js_fixture(
        "trust-grant-js-fixture",
        DECLARING_MANIFEST,
        DECLARING_PLUGIN_JS,
    );
    let native = TrustGrantNativeStub::default();
    let expected = Some(IntegrationTrustGrantCapability {
        managed_keys: vec!["session_start".into()],
    });
    assert_eq!(js.describe().integration_trust_grant, expected);
    assert_eq!(native.describe().integration_trust_grant, expected);
}

#[test]
fn codex_manifest_declares_trust_grant_capability() {
    if require_plugins().is_none() {
        return;
    }
    let codex = load_js("codex");
    assert_eq!(
        codex.integration_trust_grant_capability(),
        Some(IntegrationTrustGrantCapability {
            managed_keys: vec!["session_start".into(), "stop".into()],
        }),
        "bundled codex manifest must declare the trust grant scoped to its managed hook keys"
    );
}
