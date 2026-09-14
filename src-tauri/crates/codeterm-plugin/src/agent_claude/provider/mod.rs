//! Claude Code agent provider — facade.
//!
//! All Claude-specific logic (launch command, session enumeration, output
//! detection, settings/notification schemas, bridge-rule install) lives in
//! sibling modules. This file owns `ClaudeProvider` and the `AgentProvider`
//! trait impl, which delegates each method to the right child.

mod detection;
pub mod hook_ownership;
pub mod integration;
mod interactive;
mod launch;
pub(crate) mod launch_session;
mod permission_prompt;
mod schemas;
mod sessions;
mod usage;

#[cfg(test)]
pub(crate) use schemas::manifest_defaults;

use std::path::PathBuf;
use std::time::Duration;

use crate::provider::{
    AgentLaunchParams, AgentProvider, ModelInfo, ReadinessStrategy, ReasoningEffortInfo,
    SessionIdInjection, SessionMessage, SessionMetadata, SettingField, StarterPrompt,
    SystemPromptDelivery, TaskDelivery, UsageSnapshot, VerifySubmitStrategy,
};

use super::usage_tui::ClaudeTuiUsageParser;

static CLAUDE_TUI_USAGE_PARSER: ClaudeTuiUsageParser = ClaudeTuiUsageParser;

/// Claude Code agent provider.
pub struct ClaudeProvider;

impl ClaudeProvider {
    pub fn new() -> Self {
        crate::model_catalog::register("claude", schemas::manifest_defaults());
        let (efforts, default) = schemas::reasoning_effort_defaults();
        crate::model_catalog::register_reasoning_efforts("claude", efforts, default);
        Self
    }
}

impl AgentProvider for ClaudeProvider {
    fn id(&self) -> &str {
        "claude"
    }

    fn supports_chat_ui(&self) -> bool {
        true
    }

    fn supports_structured_prompts(&self) -> bool {
        true
    }

    fn supports_permission_auto_confirm(&self) -> bool {
        true
    }

    fn structured_pending_prompt(
        &self,
        cwd: &str,
        session_id: &str,
    ) -> Option<codeterm_types::pty::PendingPrompt> {
        sessions::structured_pending_prompt(cwd, session_id)
    }

    fn display_name(&self) -> &str {
        "Claude Code"
    }

    fn binary_name(&self) -> &str {
        "claude"
    }

    fn build_launch_command(&self, params: &AgentLaunchParams) -> String {
        launch::build_launch_command(params)
    }

    fn starter_prompt_text(&self, prompt: StarterPrompt) -> Option<String> {
        launch::starter_prompt_text(prompt).map(str::to_string)
    }

    fn session_id_injection(&self) -> SessionIdInjection {
        // Claude Code 2.1.x writes no transcript for a fresh interactive --session-id launch — let it pick its own id, we detect it after launch.
        SessionIdInjection::None
    }

    fn detect_launch_session(
        &self,
        evidence: &crate::provider::LaunchSessionEvidence,
    ) -> Option<crate::provider::LaunchSessionDetection> {
        launch_session::detect_launch_session(evidence)
    }

    fn enumerate_sessions(&self) -> Vec<SessionMetadata> {
        sessions::enumerate_sessions()
    }

    fn parse_session_messages(&self, file_path: &std::path::Path) -> Vec<SessionMessage> {
        sessions::parse_session_messages(file_path)
    }

    fn parse_interactive_events(
        &self,
        msg: &codeterm_types::agent::chat_types::RichMessage,
    ) -> Vec<codeterm_types::agent::chat_types::InteractiveElement> {
        interactive::parse_interactive_events(msg)
    }

    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::PollShellPrompt {
            timeout: Duration::from_secs(30),
        }
    }

    fn task_delivery(&self) -> TaskDelivery {
        TaskDelivery::InlineFlag
    }

    fn system_prompt_delivery(&self) -> SystemPromptDelivery {
        SystemPromptDelivery::CliFlag("--append-system-prompt-file".into())
    }

    fn verify_submit_strategy(&self) -> VerifySubmitStrategy {
        VerifySubmitStrategy::TuiInputDrained {
            // The composer swallows a submit key glued to a paste it is still
            // ingesting, which leaves the text sitting unsent.
            initial_key_delay: Duration::from_millis(100),
        }
    }

    fn detect_from_title(&self, title: &str) -> bool {
        detection::detect_from_title(title)
    }

    fn detect_from_output(&self, text: &str) -> bool {
        detection::detect_from_output(text)
    }

    fn extract_session_name(&self, screen: &str) -> Option<String> {
        detection::extract_session_name(screen)
    }

    fn screen_has_tui(&self, screen_text: &str) -> bool {
        let lower = screen_text.to_ascii_lowercase();
        lower.contains("? for shortcuts")
            || lower.contains("for shortcuts")
            || lower.contains("ctrl+l")
            || lower.contains("esc to interrupt")
            // v2.1+ idle footer ("⏵⏵ bypass permissions on (shift+tab to cycle)")
            || lower.contains("bypass permissions")
            || lower.contains("shift+tab to cycle")
            // Banner logo quadrant blocks — never in plain shell output.
            || screen_text.contains('▛')
            || screen_text.contains('▜')
    }

    fn session_file_path(&self, cwd: &str, session_id: &str) -> Option<PathBuf> {
        sessions::session_file_path(cwd, session_id)
    }

    fn session_exists(&self, cwd: &str, session_id: &str) -> bool {
        sessions::sessions_dir(cwd)
            .map(|dir| dir.join(format!("{session_id}.jsonl")).is_file())
            .unwrap_or(false)
    }

    fn detect_session_id(
        &self,
        cwd: &str,
        exclude: &[String],
        max_age: Option<Duration>,
    ) -> Option<String> {
        sessions::detect_session_id(cwd, exclude, max_age)
    }

    fn detect_session_model(&self, cwd: &str, session_id: &str) -> Option<String> {
        sessions::detect_session_model(cwd, session_id).map(|family| {
            crate::model_catalog::resolve_family_model("claude", &family).unwrap_or(family)
        })
    }

    fn detect_session_reasoning_effort(&self, cwd: &str, session_id: &str) -> Option<String> {
        sessions::detect_session_reasoning_effort(cwd, session_id)
    }

    fn reasoning_efforts(&self) -> Vec<ReasoningEffortInfo> {
        crate::model_catalog::resolve_reasoning_efforts("claude")
    }

    fn default_reasoning_effort(&self) -> Option<String> {
        crate::model_catalog::resolve_default_reasoning_effort("claude")
    }

    fn reasoning_effort_launch_args(&self, effort: &str) -> Vec<String> {
        if effort.is_empty() {
            Vec::new()
        } else {
            schemas::reasoning_effort_launch_args_format()
                .into_iter()
                .map(|arg| arg.replace("{effort}", effort))
                .collect()
        }
    }

    fn build_resume_command(&self, session_id: &str, skip_permissions: bool) -> Option<String> {
        launch::build_resume_command(session_id, skip_permissions)
    }

    fn build_resume_command_with_context(
        &self,
        params: &crate::provider::AgentResumeParams,
    ) -> Option<String> {
        launch::build_resume_command_with_context(params)
    }

    fn read_usage(&self, cwd: &str, session_id: &str) -> Option<UsageSnapshot> {
        usage::read(cwd, session_id)
    }

    fn usage_parser(&self) -> Option<&dyn crate::usage_parser::UsageParser> {
        Some(&CLAUDE_TUI_USAGE_PARSER)
    }

    fn usage_probe_command(&self) -> Option<&str> {
        Some("/usage")
    }

    fn has_event_markers(&self, chunk: &str) -> bool {
        detection::has_event_markers(chunk)
    }

    fn detect_events(&self, text: &str) -> Vec<crate::detector::AgentEvent> {
        detection::detect_claude_events(text)
    }

    fn parse_prompt(&self, text: &str) -> Option<codeterm_types::pty::PendingPrompt> {
        crate::detector::parse_permission_prompt(text)
            .map(|(question, options)| crate::prompt::build_prompt(question, options))
    }

    fn has_hooks(&self) -> bool {
        true
    }

    fn hook_event_roster(&self) -> Vec<crate::provider::HookEventBinding> {
        integration::hook_event_roster()
    }

    /// Claude TUI supports vim j/k navigation — simpler and more reliable than arrow keys.
    fn build_prompt_response(&self, option_index: usize, screen: &str) -> Vec<Vec<u8>> {
        let current = highlighted_option_index(screen).unwrap_or(0);
        let delta = option_index as isize - current as isize;
        let mut steps = Vec::new();
        let key = if delta >= 0 { b"j" } else { b"k" };
        for _ in 0..delta.unsigned_abs() {
            steps.push(key.to_vec());
        }
        steps.push(b"\r".to_vec());
        steps
    }

    fn permission_prompt_response(
        &self,
        tool_name: &str,
        prompt: &codeterm_types::pty::PendingPrompt,
        screen: &str,
    ) -> Option<crate::permission_prompt::PermissionPromptResponse> {
        let option_index = permission_prompt::affirmative_option(tool_name, prompt, screen)?;
        Some(crate::permission_prompt::PermissionPromptResponse {
            option_index,
            rule: permission_prompt::RULE.to_string(),
            steps: self.build_prompt_response(option_index, screen),
        })
    }

    fn status_bar_rows(&self) -> u16 {
        3
    }

    fn supports_agent_state_status_reconciliation(&self) -> bool {
        true
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

    fn fresh_context_command(&self) -> Option<&str> {
        Some("/new")
    }

    fn model_switch_format(&self) -> Option<&str> {
        Some("/model {model}")
    }

    fn exit_command(&self) -> Option<&str> {
        Some("/exit")
    }

    fn stop_keystroke(&self) -> Option<&str> {
        Some("\u{1b}")
    }

    fn supports_lazy_skills(&self) -> bool {
        true
    }

    fn icon_hint(&self) -> Option<&str> {
        Some("✳")
    }

    fn icon_svg(&self) -> Option<&str> {
        Some("<svg viewBox=\"0 0 100 100\" fill=\"currentColor\" xmlns=\"http://www.w3.org/2000/svg\"><path d=\"m19.6 66.5 19.7-11 .3-1-.3-.5h-1l-3.3-.2-11.2-.3L14 53l-9.5-.5-2.4-.5L0 49l.2-1.5 2-1.3 2.9.2 6.3.5 9.5.6 6.9.4L38 49.1h1.6l.2-.7-.5-.4-.4-.4L29 41l-10.6-7-5.6-4.1-3-2-1.5-2-.6-4.2 2.7-3 3.7.3.9.2 3.7 2.9 8 6.1L37 36l1.5 1.2.6-.4.1-.3-.7-1.1L33 25l-6-10.4-2.7-4.3-.7-2.6c-.3-1-.4-2-.4-3l3-4.2L28 0l4.2.6L33.8 2l2.6 6 4.1 9.3L47 29.9l2 3.8 1 3.4.3 1h.7v-.5l.5-7.2 1-8.7 1-11.2.3-3.2 1.6-3.8 3-2L61 2.6l2 2.9-.3 1.8-1.1 7.7L59 27.1l-1.5 8.2h.9l1-1.1 4.1-5.4 6.9-8.6 3-3.5L77 13l2.3-1.8h4.3l3.1 4.7-1.4 4.9-4.4 5.6-3.7 4.7-5.3 7.1-3.2 5.7.3.4h.7l12-2.6 6.4-1.1 7.6-1.3 3.5 1.6.4 1.6-1.4 3.4-8.2 2-9.6 2-14.3 3.3-.2.1.2.3 6.4.6 2.8.2h6.8l12.6 1 3.3 2 1.9 2.7-.3 2-5.1 2.6-6.8-1.6-16-3.8-5.4-1.3h-.8v.4l4.6 4.5 8.3 7.5L89 80.1l.5 2.4-1.3 2-1.4-.2-9.2-7-3.6-3-8-6.8h-.5v.7l1.8 2.7 9.8 14.7.5 4.5-.7 1.4-2.6 1-2.7-.6-5.8-8-6-9-4.7-8.2-.5.4-2.9 30.2-1.3 1.5-3 1.2-2.5-2-1.4-3 1.4-6.2 1.6-8 1.3-6.4 1.2-7.9.7-2.6v-.2H49L43 72l-9 12.3-7.2 7.6-1.7.7-3-1.5.3-2.8L24 86l10-12.8 6-7.9 4-4.6-.1-.5h-.3L17.2 77.4l-4.7.6-2-2 .2-3 1-1 8-5.5Z\"/></svg>")
    }

    fn icon_color(&self) -> Option<&str> {
        Some("#d97757")
    }

    fn cli_binary(&self) -> Option<&str> {
        Some("claude")
    }

    fn install_command(&self, platform: &str) -> Option<String> {
        match platform {
            // Anthropic's native installer is the recommended path — auto-updates in the background, no Node.js dependency. npm is now an "advanced"
            // option per https://code.claude.com/docs/en/setup.
            "macos" | "linux" => Some("curl -fsSL https://claude.ai/install.sh | bash".into()),
            "windows" => {
                Some("powershell -Command \"irm https://claude.ai/install.ps1 | iex\"".into())
            }
            _ => None,
        }
    }

    fn update_command(&self, platform: &str) -> Option<String> {
        matches!(platform, "macos" | "linux" | "windows").then(|| "claude update".to_string())
    }

    fn available_models(&self) -> Vec<ModelInfo> {
        schemas::available_models()
    }

    fn default_model(&self) -> Option<String> {
        crate::model_catalog::resolve_default("claude")
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

    fn check_integration(&self) -> serde_json::Value {
        integration::check_integration()
    }

    fn has_installable_integration(&self) -> bool {
        true
    }

    fn install_integration(
        &self,
        api_port: u16,
        options: &crate::provider::IntegrationInstallOptions,
    ) -> Result<(), String> {
        integration::install_integration(api_port, options)
    }

    fn uninstall_integration(&self) -> Result<(), String> {
        integration::uninstall_integration()
    }

    fn restore_integration(&self) -> Result<(), String> {
        integration::restore_integration()
    }

    fn ensure_integration_on_startup(&self, api_port: u16) {
        integration::ensure_integration_on_startup(api_port);
    }

    fn check_rule_status(&self) -> serde_json::Value {
        integration::check_rule_status()
    }

    fn install_rule(&self) -> Result<(), String> {
        integration::install_rule()
    }
}

fn highlighted_option_index(screen: &str) -> Option<usize> {
    let mut option_idx = 0usize;
    for line in screen.lines() {
        let trimmed = line.trim_start();
        let selected = trimmed.starts_with('❯') || trimmed.starts_with('>');
        let candidate = trimmed.trim_start_matches(['❯', '>']).trim_start();
        let digits = candidate
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>();
        if digits.is_empty() || !candidate[digits.len()..].starts_with(". ") {
            continue;
        }
        if selected {
            return Some(option_idx);
        }
        option_idx += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claude_provider_id() {
        let p = ClaudeProvider::new();
        assert_eq!(p.id(), "claude");
        assert_eq!(p.display_name(), "Claude Code");
        assert_eq!(p.binary_name(), "claude");
    }

    #[test]
    fn claude_auto_approve_flags_has_dangerously_skip() {
        let p = ClaudeProvider::new();
        assert_eq!(
            p.auto_approve_flags(),
            vec!["--dangerously-skip-permissions"]
        );
    }

    #[test]
    fn claude_compact_command() {
        let p = ClaudeProvider::new();
        assert_eq!(p.compact_command(), Some("/compact"));
    }

    #[test]
    fn claude_fresh_context_command() {
        let p = ClaudeProvider::new();
        assert_eq!(p.fresh_context_command(), Some("/new"));
    }

    #[test]
    fn claude_model_switch_command_formats_slash_model() {
        let p = ClaudeProvider::new();
        assert_eq!(
            p.model_switch_command("sonnet").as_deref(),
            Some("/model sonnet")
        );
    }

    #[test]
    fn claude_prompt_response_is_highlight_relative() {
        let p = ClaudeProvider::new();
        let screen = "Choose:\n  1. Opus\n❯ 2. Sonnet\n  3. Haiku\nEnter to select";
        assert_eq!(p.build_prompt_response(1, screen), vec![b"\r".to_vec()]);
        assert_eq!(
            p.build_prompt_response(0, screen),
            vec![b"k".to_vec(), b"\r".to_vec()]
        );
        assert_eq!(
            p.build_prompt_response(2, screen),
            vec![b"j".to_vec(), b"\r".to_vec()]
        );
    }

    #[test]
    fn claude_describe_injects_usage_tracking_section() {
        let p = ClaudeProvider::new();
        let desc = p.describe();
        let has = desc.settings_schema.iter().any(|f| matches!(
            f,
            crate::provider::SettingField::Section { title, .. } if title.starts_with("Usage Tracking")
        ));
        assert!(
            has,
            "Claude has usage_parser — describe() must auto-inject the Usage Tracking toggle"
        );
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
        // Default implementation is a no-op that returns Ok(()) — ClaudeProvider doesn't override it yet (real skill delivery comes with Phase 2).
        assert!(p.install_skills(&[]).is_ok());
        assert!(p.uninstall_skills(&[]).is_ok());
    }

    #[test]
    fn test_claude_does_not_inject_session_id() {
        // Claude 2.1.x drops the transcript when --session-id is passed to a fresh interactive launch; we detect the id instead.
        let p = ClaudeProvider::new();
        assert_eq!(
            p.session_id_injection(),
            crate::provider::SessionIdInjection::None
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
    fn claude_uses_tui_input_drained_for_submit_verification() {
        let p = ClaudeProvider::new();
        assert!(matches!(
            p.verify_submit_strategy(),
            VerifySubmitStrategy::TuiInputDrained { .. }
        ));
    }

    #[test]
    fn claude_describe_assembles_full_descriptor() {
        let p = ClaudeProvider::new();
        let desc = p.describe();
        assert_eq!(desc.id, "claude");
        assert_eq!(desc.display_name, "Claude Code");
        assert_eq!(desc.icon_hint.as_deref(), Some("✳"));
        assert_eq!(desc.default_model.as_deref(), Some("sonnet"));
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
        let decoded: crate::provider::ProviderDescriptor =
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
