//! A minimal `AgentProvider` for tests, now that the native codex/opencode
//! providers are gone (everything-as-plugins). Lets registry/dispatch tests
//! register several distinct providers without a real plugin VM.
#![cfg(test)]

use std::path::PathBuf;
use std::time::Duration;

use super::provider::{
    AgentLaunchParams, AgentProvider, ReadinessStrategy, ReasoningEffortInfo, SystemPromptDelivery,
    TaskDelivery,
};

pub struct MockProvider {
    id: String,
    supports_chat_ui: bool,
    reasoning_efforts: Vec<ReasoningEffortInfo>,
    system_prompt_delivery: SystemPromptDelivery,
    status_bar_rows: u16,
    update_command: Option<String>,
}

impl MockProvider {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            supports_chat_ui: false,
            reasoning_efforts: Vec::new(),
            system_prompt_delivery: SystemPromptDelivery::External,
            status_bar_rows: 0,
            update_command: None,
        }
    }

    /// Declares a chrome band, so a test can round-trip it through the registry
    /// instead of comparing against a baked number.
    pub fn with_status_bar_rows(id: &str, rows: u16) -> Self {
        Self {
            status_bar_rows: rows,
            ..Self::new(id)
        }
    }

    pub fn with_chat_ui(id: &str) -> Self {
        Self {
            supports_chat_ui: true,
            ..Self::new(id)
        }
    }

    /// Declares a system-prompt flag but no resume re-application — the
    /// retain class, where the host has content to hand over and must not.
    pub fn with_system_prompt_flag(id: &str, flag: &str) -> Self {
        Self {
            system_prompt_delivery: SystemPromptDelivery::CliFlag(flag.to_string()),
            ..Self::new(id)
        }
    }

    /// Fixture with declared reasoning-effort levels and a scriptable launch flag.
    pub fn with_reasoning_efforts(id: &str, levels: &[&str]) -> Self {
        Self {
            reasoning_efforts: levels
                .iter()
                .map(|l| ReasoningEffortInfo {
                    id: (*l).to_string(),
                    display_name: (*l).to_string(),
                    description: None,
                })
                .collect(),
            ..Self::new(id)
        }
    }

    pub fn with_update_command(id: &str, command: &str) -> Self {
        Self {
            update_command: Some(command.to_string()),
            ..Self::new(id)
        }
    }
}

impl AgentProvider for MockProvider {
    fn id(&self) -> &str {
        &self.id
    }
    fn display_name(&self) -> &str {
        &self.id
    }
    fn build_launch_command(&self, params: &AgentLaunchParams) -> String {
        let mut cmd = format!("{}-cli", self.id);
        if let Some(args) = &params.args {
            for a in args {
                cmd.push(' ');
                cmd.push_str(a);
            }
        }
        cmd
    }
    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::FixedDelay {
            duration: Duration::from_millis(0),
        }
    }
    fn supports_chat_ui(&self) -> bool {
        self.supports_chat_ui
    }
    fn status_bar_rows(&self) -> u16 {
        self.status_bar_rows
    }
    fn reasoning_efforts(&self) -> Vec<ReasoningEffortInfo> {
        self.reasoning_efforts.clone()
    }
    fn reasoning_effort_launch_args(&self, effort: &str) -> Vec<String> {
        if effort.is_empty() {
            return Vec::new();
        }
        vec!["--effort".into(), effort.into()]
    }
    fn task_delivery(&self) -> TaskDelivery {
        TaskDelivery::PtyInput
    }
    fn system_prompt_delivery(&self) -> SystemPromptDelivery {
        self.system_prompt_delivery.clone()
    }
    fn update_command(&self, _platform: &str) -> Option<String> {
        self.update_command.clone()
    }
    fn detect_from_title(&self, title: &str) -> bool {
        title.to_lowercase().contains(&self.id)
    }
    fn detect_from_output(&self, text: &str) -> bool {
        text.to_lowercase().contains(&self.id)
    }
    fn session_file_path(&self, _cwd: &str, _session_id: &str) -> Option<PathBuf> {
        None
    }
    fn build_resume_command(&self, session_id: &str, _skip_permissions: bool) -> Option<String> {
        Some(format!("{}-cli --resume {session_id}", self.id))
    }
    fn detect_session_id(
        &self,
        _cwd: &str,
        _exclude: &[String],
        _max_age: Option<Duration>,
    ) -> Option<String> {
        None
    }
}
