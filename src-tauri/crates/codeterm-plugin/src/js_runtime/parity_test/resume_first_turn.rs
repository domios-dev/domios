use super::*;

struct ResumeFirstTurnNative;

#[async_trait::async_trait]
impl AgentProvider for ResumeFirstTurnNative {
    fn id(&self) -> &str {
        "resume-first-turn-fixture"
    }

    fn display_name(&self) -> &str {
        "Resume First Turn Fixture"
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

    fn resume_binds_on_first_user_turn(&self) -> bool {
        true
    }
}

#[test]
fn resume_binds_on_first_turn_capability_native_js_parity() {
    let manifest_json = r#"{
        "id": "resume-first-turn-fixture",
        "version": "0.1.0",
        "displayName": "Resume First Turn Fixture",
        "spawn": {
            "readinessStrategy": {"kind": "fixed_delay", "durationMs": 0},
            "taskDelivery": "pty_input",
            "verifySubmit": {"kind": "none"},
            "resumeBindsOnFirstUserTurn": true
        }
    }"#;
    let manifest: super::super::PluginManifest = serde_json::from_str(manifest_json).unwrap();
    let vm = super::super::PluginVm::from_source_with(
        "resume-first-turn-fixture",
        "module.exports.default = {};".into(),
        Some(manifest_json.into()),
    )
    .unwrap();
    let js = JsProvider::new(manifest, std::sync::Arc::new(vm));
    let native = ResumeFirstTurnNative;

    assert!(native.resume_binds_on_first_user_turn());
    assert_eq!(
        js.resume_binds_on_first_user_turn(),
        native.resume_binds_on_first_user_turn()
    );

    if require_plugins().is_some() {
        for id in ["codex", "opencode"] {
            assert!(
                load_js(id).resume_binds_on_first_user_turn(),
                "{id} manifest declares first-turn resume binding"
            );
        }
    }
}
