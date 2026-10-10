use super::*;

struct IdentityNative;

#[async_trait::async_trait]
impl AgentProvider for IdentityNative {
    fn id(&self) -> &str {
        "identity-fixture"
    }
    fn display_name(&self) -> &str {
        "Identity fixture"
    }
    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }
    fn readiness_strategy(&self) -> crate::provider::ReadinessStrategy {
        crate::provider::ReadinessStrategy::PollShellPrompt {
            timeout: std::time::Duration::ZERO,
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
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
    fn session_model_identity_path(&self, cwd: &str, session_id: &str) -> Option<PathBuf> {
        (session_id == "owned").then(|| {
            PathBuf::from(cwd)
                .join("summary.json")
                .canonicalize()
                .unwrap()
        })
    }
}

#[test]
fn session_model_identity_path_native_js_parity_and_confinement() {
    use std::sync::Arc;
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("summary.json"), "{}").unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    let manifest = serde_json::json!({
        "id": "identity-fixture", "version": "0.1.0", "displayName": "Fixture",
        "sessions": {"rootDir": root.path().to_string_lossy(), "cwdEncoding": "none"}
    });
    let script = format!(
        r#"module.exports.default = {{sessionModelIdentityPath(cwd, sid) {{
        return sid === 'owned' ? cwd + '/summary.json' : sid === 'escape' ? {} : null;
    }} }};"#,
        serde_json::to_string(&outside.path().to_string_lossy()).unwrap()
    );
    let vm = super::super::super::PluginVm::from_source_with(
        "identity-fixture",
        script,
        Some(manifest.to_string()),
    )
    .unwrap();
    let js = JsProvider::new(serde_json::from_value(manifest).unwrap(), Arc::new(vm));
    let cwd = root.path().to_str().unwrap();
    for sid in ["owned", "foreign"] {
        assert_eq!(
            js.session_model_identity_path(cwd, sid),
            IdentityNative.session_model_identity_path(cwd, sid)
        );
    }
    assert!(js.session_model_identity_path(cwd, "escape").is_none());
    let absent = load_inline_js("identity-absent", "module.exports.default = {};");
    assert_eq!(
        absent.session_model_identity_path(cwd, "owned"),
        DiscoverModelsNativeStub.session_model_identity_path(cwd, "owned")
    );
}
