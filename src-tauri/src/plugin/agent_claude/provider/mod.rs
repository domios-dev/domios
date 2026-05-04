//! Claude Code agent provider — facade.
//!
//! All Claude-specific logic (launch command, session enumeration, output
//! detection, settings/notification schemas, bridge-rule install) lives in
//! sibling modules. This file owns `ClaudeProvider` and the `AgentProvider`
//! trait impl, which delegates each method to the right child.

mod detection;
mod launch;
mod schemas;
mod sessions;

use std::path::PathBuf;
use std::time::Duration;

use crate::agent::provider::{
    AgentLaunchParams, AgentProvider, ModelInfo, ReadinessStrategy, SessionIdInjection,
    SessionMessage, SessionMetadata, SettingField, TaskDelivery,
};

/// Claude Code agent provider.
pub struct ClaudeProvider;

impl ClaudeProvider {
    pub fn new() -> Self {
        Self
    }
}

impl AgentProvider for ClaudeProvider {
    fn id(&self) -> &str {
        "claude"
    }

    fn display_name(&self) -> &str {
        "Claude Code"
    }

    fn build_launch_command(&self, params: &AgentLaunchParams) -> String {
        launch::build_launch_command(params)
    }

    fn session_id_injection(&self) -> SessionIdInjection {
        SessionIdInjection::FreshAndFork
    }

    fn enumerate_sessions(&self) -> Vec<SessionMetadata> {
        sessions::enumerate_sessions()
    }

    fn parse_session_messages(&self, file_path: &std::path::Path) -> Vec<SessionMessage> {
        sessions::parse_session_messages(file_path)
    }

    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::PollShellPrompt {
            timeout: Duration::from_secs(30),
        }
    }

    fn task_delivery(&self) -> TaskDelivery {
        TaskDelivery::InlineFlag
    }

    fn detect_from_title(&self, title: &str) -> bool {
        detection::detect_from_title(title)
    }

    fn detect_from_output(&self, text: &str) -> bool {
        detection::detect_from_output(text)
    }

    fn session_file_path(&self, cwd: &str, session_id: &str) -> Option<PathBuf> {
        sessions::session_file_path(cwd, session_id)
    }

    fn detect_session_id(
        &self,
        cwd: &str,
        exclude: &[String],
        max_age: Option<Duration>,
    ) -> Option<String> {
        sessions::detect_session_id(cwd, exclude, max_age)
    }

    fn build_resume_command(&self, session_id: &str, skip_permissions: bool) -> Option<String> {
        launch::build_resume_command(session_id, skip_permissions)
    }

    fn has_event_markers(&self, chunk: &str) -> bool {
        detection::has_event_markers(chunk)
    }

    fn detect_events(&self, text: &str) -> Vec<crate::agent::detector::AgentEvent> {
        detection::detect_claude_events(text)
    }

    fn parse_prompt(&self, text: &str) -> Option<(String, Vec<(String, String)>)> {
        crate::agent::detector::parse_permission_prompt(text)
    }

    fn has_hooks(&self) -> bool {
        true
    }

    /// Claude TUI supports vim j/k navigation — simpler and more reliable than arrow keys.
    fn build_prompt_response(&self, option_index: usize) -> Vec<Vec<u8>> {
        let mut steps = Vec::new();
        for _ in 0..option_index {
            steps.push(b"j".to_vec());
        }
        steps.push(b"\r".to_vec());
        steps
    }

    fn status_bar_rows(&self) -> u16 {
        3
    }

    fn is_output_noise(&self, text: &str) -> bool {
        detection::is_output_noise(text)
    }

    fn auto_approve_flags(&self) -> Vec<String> {
        vec!["--dangerously-skip-permissions".to_string()]
    }

    fn compact_command(&self) -> Option<&str> {
        Some("/compact")
    }

    fn model_switch_command(&self, model: &str) -> Option<String> {
        Some(format!("/model {}", model))
    }

    fn exit_command(&self) -> Option<&str> {
        Some("/exit")
    }

    fn supports_lazy_skills(&self) -> bool {
        true
    }

    fn icon_hint(&self) -> Option<&str> {
        Some("✳")
    }

    fn cli_binary(&self) -> Option<&str> {
        Some("claude")
    }

    fn install_command(&self, platform: &str) -> Option<String> {
        match platform {
            "macos" | "linux" => Some("npm install -g @anthropic-ai/claude-code".into()),
            "windows" => Some("npm install -g @anthropic-ai/claude-code".into()),
            _ => None,
        }
    }

    fn available_models(&self) -> Vec<ModelInfo> {
        schemas::available_models()
    }

    fn default_model(&self) -> Option<&str> {
        Some("sonnet")
    }

    fn preserves_context_on_resume(&self) -> bool {
        // Claude replays the JSONL session file on `--resume`, preserving context.
        true
    }

    fn notifications_schema(&self) -> Vec<SettingField> {
        schemas::notifications_schema()
    }

    fn settings_schema(&self) -> Vec<SettingField> {
        schemas::settings_schema()
    }

    fn install_bridge_rule(&self, payload: &str) -> Result<(), String> {
        schemas::install_bridge_rule(payload)
    }

    fn uninstall_bridge_rule(&self) -> Result<(), String> {
        schemas::uninstall_bridge_rule()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claude_provider_id() {
        let p = ClaudeProvider::new();
        assert_eq!(p.id(), "claude");
        assert_eq!(p.display_name(), "Claude Code");
    }

    #[test]
    fn claude_auto_approve_flags_has_dangerously_skip() {
        let p = ClaudeProvider::new();
        assert_eq!(p.auto_approve_flags(), vec!["--dangerously-skip-permissions"]);
    }

    #[test]
    fn claude_compact_command() {
        let p = ClaudeProvider::new();
        assert_eq!(p.compact_command(), Some("/compact"));
    }

    #[test]
    fn claude_model_switch_command_formats_slash_model() {
        let p = ClaudeProvider::new();
        assert_eq!(p.model_switch_command("sonnet").as_deref(), Some("/model sonnet"));
    }

    #[test]
    fn claude_exit_command() {
        let p = ClaudeProvider::new();
        assert_eq!(p.exit_command(), Some("/exit"));
    }

    #[test]
    fn claude_supports_lazy_skills() {
        let p = ClaudeProvider::new();
        assert!(p.supports_lazy_skills());
    }

    #[test]
    fn claude_install_skills_default_noop_is_ok() {
        let p = ClaudeProvider::new();
        // Default implementation is a no-op that returns Ok(()) — ClaudeProvider
        // doesn't override it yet (real skill delivery comes with Phase 2).
        assert!(p.install_skills(&[]).is_ok());
        assert!(p.uninstall_skills(&[]).is_ok());
    }

    #[test]
    fn test_session_id_injection_supported_for_claude() {
        let p = ClaudeProvider::new();
        assert_eq!(
            p.session_id_injection(),
            crate::agent::provider::SessionIdInjection::FreshAndFork
        );
    }

    #[test]
    fn test_readiness_strategy() {
        let p = ClaudeProvider::new();
        match p.readiness_strategy() {
            ReadinessStrategy::PollShellPrompt { timeout } => {
                assert_eq!(timeout, Duration::from_secs(30));
            }
            _ => panic!("Expected PollShellPrompt"),
        }
    }

    #[test]
    fn claude_describe_assembles_full_descriptor() {
        let p = ClaudeProvider::new();
        let desc = p.describe();
        assert_eq!(desc.id, "claude");
        assert_eq!(desc.display_name, "Claude Code");
        assert_eq!(desc.icon_hint.as_deref(), Some("✳"));
        assert_eq!(desc.default_model.as_deref(), Some("sonnet"));
        assert!(desc.preserves_context_on_resume);
        assert!(desc.supports_lazy_skills);
        assert!(desc.has_hooks);
        assert!(!desc.settings_schema.is_empty());
    }

    #[test]
    fn claude_settings_schema_round_trips_as_json() {
        // Snapshot/golden — catches accidental schema breakage.
        let p = ClaudeProvider::new();
        let desc = p.describe();
        let json = serde_json::to_string(&desc).expect("serialize");
        let decoded: crate::agent::provider::ProviderDescriptor =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(desc, decoded);
    }

    #[test]
    fn claude_describe_includes_notifications_schema() {
        let p = ClaudeProvider::new();
        let desc = p.describe();
        assert!(!desc.notifications_schema.is_empty());
    }
}
