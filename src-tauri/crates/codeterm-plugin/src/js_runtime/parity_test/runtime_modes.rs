use super::*;
use crate::provider::*;

struct Native;
#[async_trait::async_trait]
impl AgentProvider for Native {
    fn id(&self) -> &str {
        "fixture"
    }
    fn display_name(&self) -> &str {
        "Fixture"
    }
    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
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
    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::PollShellPrompt {
            timeout: std::time::Duration::from_secs(30),
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
    fn agent_runtime(&self) -> Option<crate::agent_runtime::AgentRuntimeCapability> {
        Some(serde_json::from_value(serde_json::json!({
            "runtime":"stdio-jsonrpc", "command":{"binary":"fixture-engine","args":[]},
            "runtimeModes":[{"id":"auto","displayName":"Auto"},{"id":"full-access","displayName":"Full access"}],
            "requestMethods":{"control":["fixture/settings"]}, "notificationMethods":[]
        })).unwrap())
    }
}

#[test]
fn parity_runtime_modes_native_js_describe() {
    let manifest = crate::js_runtime::PluginManifest::from_json_str(&serde_json::json!({
        "id":"fixture", "version":"1.0.0", "displayName":"Fixture",
        "capabilities":{"agentProvider":true,"chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},
            "agentRuntime":{"runtime":"stdio-jsonrpc","command":{"binary":"fixture-engine","args":[]},
                "runtimeModes":[{"id":"auto","displayName":"Auto"},{"id":"full-access","displayName":"Full access"}],
                "requestMethods":{"control":["fixture/settings"]},"notificationMethods":[]}},
        "permissions":{"subprocess":{"allow":["fixture-engine"]}}
    }).to_string()).unwrap();
    let vm =
        crate::js_runtime::PluginVm::from_source("fixture", "module.exports={};".into()).unwrap();
    let js = crate::js_runtime::JsProvider::new(manifest, std::sync::Arc::new(vm));
    assert_eq!(Native.agent_runtime(), js.agent_runtime());
    assert_eq!(Native.describe().agent_runtime, js.describe().agent_runtime);
    assert_eq!(js.describe().agent_runtime.unwrap().runtime_modes.len(), 2);
    for descriptor in [Native.describe(), js.describe()] {
        assert!(!descriptor
            .settings_schema
            .iter()
            .any(|section| matches!(section,
            SettingField::Section { fields, .. } if fields.iter().any(|field| matches!(field,
                SettingField::Toggle { key, .. } if key == "structured_control_enabled")))));
        assert!(serde_json::to_value(descriptor)
            .unwrap()
            .get("structured_control")
            .is_none());
    }
    let absent = load_inline_js("absent", "");
    assert_eq!(absent.agent_runtime(), None);
}
