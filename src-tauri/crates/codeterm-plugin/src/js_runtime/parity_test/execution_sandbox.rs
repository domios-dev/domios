use super::*;
use crate::provider::{ExecutionSandboxCapability, ExecutionSandboxMode};

pub(super) fn fixture_capability() -> ExecutionSandboxCapability {
    ExecutionSandboxCapability {
        platforms: vec!["fixture-os".into()],
        read_only: Some(ExecutionSandboxMode {
            launch_args: vec!["opaque-read-arg".into()],
            runtime_mode: Some("opaque-read-mode".into()),
        }),
        mutating: None,
        runtime_read_only_writable_roots_mode: Some("opaque-root-exception".into()),
        unsupported_reason: Some("opaque-gap".into()),
        forbidden_argument_prefixes: vec!["opaque-deny".into()],
    }
}

#[test]
fn execution_sandbox_native_js_parity() {
    let native = NativeSandbox;
    let capability = native.execution_sandbox_capability().unwrap();
    let manifest_json = serde_json::json!({
        "id":"sandbox-fixture", "version":"0.1.0", "displayName":"Fixture",
        "spawn":{"readinessStrategy":{"kind":"fixed_delay","durationMs":0},
        "taskDelivery":"pty_input","verifySubmit":{"kind":"none"}, "executionSandbox":capability}
    })
    .to_string();
    let manifest = serde_json::from_str(&manifest_json).unwrap();
    let vm = super::super::PluginVm::from_source_with(
        "sandbox-fixture",
        String::new(),
        Some(manifest_json),
    )
    .unwrap();
    let js = JsProvider::new(manifest, std::sync::Arc::new(vm));
    assert_eq!(
        js.execution_sandbox_capability(),
        native.execution_sandbox_capability()
    );
    for intent in [
        codeterm_types::plan::ExecutionIntent::ReadOnly,
        codeterm_types::plan::ExecutionIntent::Mutating,
    ] {
        assert_eq!(
            js.execution_sandbox_capability()
                .unwrap()
                .resolve(&intent, "fixture-os", true),
            capability.resolve(&intent, "fixture-os", true)
        );
    }
    let absent = load_inline_js("absent-sandbox", "");
    assert_eq!(absent.execution_sandbox_capability(), None);
}
