use super::*;
use std::sync::Arc;

struct NativeControlKeys(bool);

#[async_trait::async_trait]
impl AgentProvider for NativeControlKeys {
    fn id(&self) -> &str {
        "control-fixture"
    }
    fn display_name(&self) -> &str {
        "Control Fixture"
    }
    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }
    fn readiness_strategy(&self) -> crate::provider::ReadinessStrategy {
        DiscoverModelsNativeStub.readiness_strategy()
    }
    fn task_delivery(&self) -> crate::provider::TaskDelivery {
        DiscoverModelsNativeStub.task_delivery()
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
    fn uses_win32_control_keys(&self) -> bool {
        self.0
    }
}

#[test]
fn win32_control_keys_native_js_parity_and_default() {
    for declared in [false, true] {
        let json =
            serde_json::json!({"id":"control-fixture","version":"0.1.0","displayName":"Fixture",
            "commands":{"usesWin32ControlKeys":declared}})
            .to_string();
        let manifest: super::super::PluginManifest = serde_json::from_str(&json).unwrap();
        let vm = super::super::PluginVm::from_source_with(
            "control-fixture",
            "module.exports.default = {};".into(),
            Some(json),
        )
        .unwrap();
        let js = JsProvider::new(manifest, Arc::new(vm));
        assert_eq!(
            NativeControlKeys(declared).uses_win32_control_keys(),
            js.uses_win32_control_keys()
        );
    }
    assert!(!DiscoverModelsNativeStub.uses_win32_control_keys());
    assert!(
        !load_inline_js("undeclared-keys", "module.exports.default = {};")
            .uses_win32_control_keys()
    );
}
