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
    fn structured_control(&self) -> Option<StructuredControl> {
        Some(StructuredControl {
            protocol: "codex-app-server".into(),
            min_version: "0.159.0".into(),
        })
    }
}

#[test]
fn parity_structured_control_native_js_describe() {
    let js = load_js("codex");
    assert_eq!(Native.structured_control(), js.structured_control());
    assert_eq!(
        Native.describe().structured_control,
        js.describe().structured_control
    );
    for descriptor in [Native.describe(), js.describe()] {
        assert!(descriptor.settings_schema.iter().any(|section| matches!(section,
            SettingField::Section { fields, .. } if fields.iter().any(|field| matches!(field,
                SettingField::Toggle { key, default: true, .. } if key == "structured_control_enabled")))));
    }
    let absent = load_inline_js("absent", "");
    assert_eq!(absent.structured_control(), None);
}
