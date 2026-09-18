//! Provider parity for the `usageSources` capability: a provider whose usage lives
//! in a log shared by many sessions declares a session-scoped source, and the JS
//! bridge must return the same spec the native override does.

use std::path::PathBuf;

use super::*;
use crate::provider::UsageSourceSpec;

struct NativeStub;

#[async_trait::async_trait]
impl AgentProvider for NativeStub {
    fn id(&self) -> &str {
        "usage-sources-fixture"
    }
    fn display_name(&self) -> &str {
        "Usage Sources Fixture"
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
    fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
        None
    }
    fn usage_sources(&self, cwd: &str, session_id: &str) -> Vec<UsageSourceSpec> {
        vec![UsageSourceSpec {
            path: PathBuf::from(format!("{cwd}/opencode.db")),
            session_id: Some(session_id.to_string()),
        }]
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
}

#[test]
fn usage_sources_native_js_parity() {
    let native = NativeStub;
    let js = load_inline_js(
        "usage-sources-fixture",
        r#"module.exports.default = {
            usageSources(cwd, sessionId) {
                return [{ path: cwd + "/opencode.db", sessionId }];
            }
        };"#,
    );
    for (cwd, sid) in [("/repo", "ses_a"), ("/other", "ses_b")] {
        assert_eq!(
            native.usage_sources(cwd, sid),
            js.usage_sources(cwd, sid),
            "JS and native agree for {sid}"
        );
    }

    let plain = load_inline_js("usage-sources-plain", "module.exports.default = {};");
    assert_eq!(
        plain.usage_sources("/repo", "ses_a"),
        Vec::<UsageSourceSpec>::new(),
        "no export and no session file leaves the default empty"
    );
}
