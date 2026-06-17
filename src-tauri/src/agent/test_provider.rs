//! A minimal `AgentProvider` for tests, now that the native codex/opencode
//! providers are gone (everything-as-plugins). Lets registry/dispatch tests
//! register several distinct providers without a real plugin VM.
#![cfg(test)]

use std::path::PathBuf;
use std::time::Duration;

use super::provider::{AgentLaunchParams, AgentProvider, ReadinessStrategy, TaskDelivery};

pub struct MockProvider {
    id: String,
}

impl MockProvider {
    pub fn new(id: &str) -> Self {
        Self { id: id.to_string() }
    }
}

impl AgentProvider for MockProvider {
    fn id(&self) -> &str {
        &self.id
    }
    fn display_name(&self) -> &str {
        &self.id
    }
    fn build_launch_command(&self, _params: &AgentLaunchParams) -> String {
        format!("{}-cli", self.id)
    }
    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::FixedDelay { duration: Duration::from_millis(0) }
    }
    fn task_delivery(&self) -> TaskDelivery {
        TaskDelivery::PtyInput
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
    fn detect_session_id(&self, _cwd: &str, _exclude: &[String], _max_age: Option<Duration>) -> Option<String> {
        None
    }
    fn detect_launched_session_id(
        &self,
        _cwd: &str,
        _exclude: &[String],
        _max_age: Option<Duration>,
        injected_session_id: Option<&str>,
    ) -> Option<String> {
        injected_session_id.map(|s| s.to_string())
    }
    fn detect_started_session(
        &self,
        _cwd: &str,
        _exclude: &[String],
        _max_age: Option<Duration>,
        injected_session_id: Option<&str>,
        _screen_text: Option<&str>,
    ) -> Option<String> {
        injected_session_id.map(|s| s.to_string())
    }
}
