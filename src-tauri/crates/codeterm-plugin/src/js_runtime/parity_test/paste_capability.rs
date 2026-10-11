use std::sync::Arc;

use super::*;
use crate::provider::{PasteCapability, ReadinessStrategy, TaskDelivery};

struct NativePasteFixture(bool, Option<crate::provider::MultilinePasteStrategy>);

#[async_trait::async_trait]
impl AgentProvider for NativePasteFixture {
    fn id(&self) -> &str {
        "paste-fixture"
    }
    fn display_name(&self) -> &str {
        "Paste Fixture"
    }
    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }
    fn readiness_strategy(&self) -> ReadinessStrategy {
        DiscoverModelsNativeStub.readiness_strategy()
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
    fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
        None
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
    fn paste_capability(&self) -> Option<PasteCapability> {
        Some(PasteCapability {
            supports_bracketed_paste: self.0,
            multiline: self.1,
            untrusted_above_lines: 0,
            typed_follow_up: String::new(),
            requires_typed_instruction_for_routed_messages: false,
        })
    }
}

#[test]
fn bracketed_paste_support_native_js_parity() {
    for (declaration, expected, multiline) in [
        (
            serde_json::json!({"supportsBracketedPaste": false}),
            false,
            None,
        ),
        (
            serde_json::json!({"supportsBracketedPaste": true}),
            true,
            None,
        ),
        (serde_json::json!({}), true, None),
        (
            serde_json::json!({"supportsBracketedPaste": false, "multiline": "tagged_block"}),
            false,
            Some(crate::provider::MultilinePasteStrategy::TaggedBlock),
        ),
    ] {
        let raw = serde_json::json!({
            "id": "paste-fixture", "version": "0.1.0", "displayName": "Paste Fixture",
            "spawn": {
                "paste": declaration,
                "readinessStrategy": {"kind": "fixed_delay", "durationMs": 0},
                "taskDelivery": "pty_input", "verifySubmit": {"kind": "none"}
            }
        })
        .to_string();
        let manifest = serde_json::from_str(&raw).unwrap();
        let vm = super::super::PluginVm::from_source_with(
            "paste-fixture",
            "module.exports.default = {};".into(),
            Some(raw),
        )
        .unwrap();
        let js = JsProvider::new(manifest, Arc::new(vm));
        let native = NativePasteFixture(expected, multiline);
        assert_eq!(js.paste_capability(), native.paste_capability());
    }
}
