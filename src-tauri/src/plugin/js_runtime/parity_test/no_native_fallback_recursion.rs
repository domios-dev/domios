use super::*;
use crate::agent::provider::{AgentLaunchParams, ReadinessStrategy, TaskDelivery};
use crate::agent::registry::AgentRegistry;
use crate::plugin::js_runtime::vm::PluginVm;
use crate::plugin::js_runtime::PluginManifest;
use std::sync::Arc;

/// The capability surface that delegates through `native()` and was the
/// recursion vector. Returned as a tuple so native and JS must agree.
fn capability_defaults(p: &dyn AgentProvider) -> (bool, bool, bool) {
    (
        p.uses_structured_chat(),
        p.supports_chat_ui(),
        p.session_jsonl_path("/tmp/x", "ses-1").is_some(),
    )
}

/// A native provider that implements none of the chat capabilities — the
/// control: trait defaults, no fallback, must simply return defaults.
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
    fn detect_launched_session_id(
        &self,
        _: &str,
        _: &[String],
        _: Option<std::time::Duration>,
        _: Option<&str>,
    ) -> Option<String> {
        None
    }
    fn detect_started_session(
        &self,
        _: &str,
        _: &[String],
        _: Option<std::time::Duration>,
        _: Option<&str>,
        _: Option<&str>,
    ) -> Option<String> {
        None
    }
}

/// A JS drop-in whose default export implements NOTHING capability-related,
/// so every chat/session method falls through to the `native()` path.
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
    // Native control: register a JS-only provider TWICE (authoritative pass
    // + hot-reload seeding tick) — exactly the boot sequence that armed the
    // overflow on ROG. With the #16 fix the displaced JS provider is never
    // recorded as a native fallback.
    let reg = Arc::new(AgentRegistry::new());
    let mut a = js_only("rec-test");
    a.set_registry(reg.clone());
    reg.register_provider(Box::new(a));
    let mut b = js_only("rec-test");
    b.set_registry(reg.clone());
    reg.register_provider(Box::new(b));

    assert!(
        reg.native_fallback("rec-test").is_none(),
        "a JS-only provider must never become its own native fallback"
    );

    // If the recursion regressed, these calls overflow the test binary
    // rather than returning. Reaching the assert at all proves termination.
    let js_caps = reg
        .with_provider("rec-test", |p| capability_defaults(p))
        .expect("js provider present");

    let native_caps = capability_defaults(&NativeStub);

    assert_eq!(
        js_caps, native_caps,
        "JS path must match the native default ({native_caps:?}) without recursing"
    );
    assert_eq!(js_caps, (false, false, false));
}
