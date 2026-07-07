use super::*;
use crate::host_traits::PluginProviderRegistrar;
use crate::js_runtime::vm::PluginVm;
use crate::js_runtime::PluginManifest;
use crate::provider::{AgentLaunchParams, AgentProvider, ReadinessStrategy, TaskDelivery};
use std::sync::Arc;

/// A registrar stub whose native_fallback always returns None — JS providers
/// registered here can never become their own native fallback.
struct NullRegistrar;
impl PluginProviderRegistrar for NullRegistrar {
    fn register_provider(&self, _: Box<dyn AgentProvider>) {}
    fn unregister_provider(&self, _: &str) -> bool {
        false
    }
    fn native_fallback(&self, _: &str) -> Option<Arc<dyn AgentProvider>> {
        None
    }
}

/// The capability surface that delegates through `native()`.
fn capability_defaults(p: &dyn AgentProvider) -> (bool, bool, bool) {
    (
        p.uses_structured_chat(),
        p.supports_chat_ui(),
        p.session_jsonl_path("/tmp/x", "ses-1").is_some(),
    )
}

struct NativeStub;
impl AgentProvider for NativeStub {
    fn id(&self) -> &str {
        "native-stub"
    }
    fn display_name(&self) -> &str {
        "Native Stub"
    }
    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }
    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::FixedDelay {
            duration: std::time::Duration::from_secs(0),
        }
    }
    fn task_delivery(&self) -> TaskDelivery {
        TaskDelivery::PtyInput
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
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
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

fn js_only(id: &str) -> JsProvider {
    let plugin_js = "module.exports.default = {};".to_string();
    let manifest_json = format!(
        r#"{{"id":"{id}","version":"0.1.0","displayName":"X",
                "spawn":{{"readinessStrategy":{{"kind":"fixed_delay","durationMs":0}},
                         "taskDelivery":"pty_input","verifySubmit":{{"kind":"none"}}}}}}"#
    );
    let manifest: PluginManifest = serde_json::from_str(&manifest_json).unwrap();
    let vm = PluginVm::from_source_with(id, plugin_js, Some(manifest_json)).unwrap();
    JsProvider::new(manifest, Arc::new(vm))
}

#[test]
fn native_and_js_agree_and_neither_recurses() {
    let reg: Arc<dyn PluginProviderRegistrar> = Arc::new(NullRegistrar);

    let mut p = js_only("rec-test");
    p.set_registrar(reg.clone());

    // native_fallback returns None → native() returns None → no recursion possible.
    assert!(
        reg.native_fallback("rec-test").is_none(),
        "a JS-only provider must never become its own native fallback"
    );

    // Capability defaults must match the native trait defaults without recursing.
    let js_caps = capability_defaults(&p);
    let native_caps = capability_defaults(&NativeStub);

    assert_eq!(
        js_caps, native_caps,
        "JS path must match native default ({native_caps:?}) without recursing"
    );
    assert_eq!(js_caps, (false, false, false));
}
