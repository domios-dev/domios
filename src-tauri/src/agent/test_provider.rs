//! A minimal `AgentProvider` for tests, now that the native codex/opencode
//! providers are gone (everything-as-plugins). Lets registry/dispatch tests
//! register several distinct providers without a real plugin VM.
#![cfg(test)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

use super::provider::{
    usage_sources_from_session_file, AgentLaunchParams, AgentProvider, AgentResumeParams,
    LaunchArtifactSweepReport, LaunchArtifactSweepRequest, ModelInfo, ProviderReadinessStrategy,
    ReadinessStrategy, ReasoningEffortInfo, SystemPromptDelivery, TaskDelivery, UsageSourceSpec,
};

pub struct MockProvider {
    id: String,
    launch_binary: Option<String>,
    supports_chat_ui: bool,
    reasoning_efforts: Vec<ReasoningEffortInfo>,
    models: Vec<ModelInfo>,
    system_prompt_delivery: SystemPromptDelivery,
    status_bar_rows: u16,
    update_command: Option<String>,
    directory_trust_response: Option<codeterm_plugin::provider::DirectoryTrustPromptResponse>,
    classified_prompt_auto_response: Option<codeterm_plugin::provider::ClassifiedPromptOptionRole>,
    tab_state_classification: Option<codeterm_plugin::tab_state::TabStateClassification>,
    parsed_prompt: Option<codeterm_types::pty::PendingPrompt>,
    parsed_directory_trust_prompt: Option<codeterm_types::pty::PendingPrompt>,
    sessions: std::collections::HashSet<String>,
    session_file: Option<PathBuf>,
    session_models: std::collections::HashMap<String, String>,
    session_reasoning_efforts: std::collections::HashMap<String, String>,
    usage_source: Option<UsageSourceSpec>,
    empty_launch_command: bool,
    empty_launch_command_when_task_present: bool,
    task_delivery: TaskDelivery,
    session_starts_on_first_user_turn: bool,
    provider_readiness_strategy: ProviderReadinessStrategy,
    launch_artifact_sweep_log: Option<Arc<Mutex<Vec<LaunchArtifactSweepRequest>>>>,
    launch_artifact_release_log: Option<Arc<Mutex<Vec<(String, String)>>>>,
    resume_context_log: Option<Arc<Mutex<Vec<AgentResumeParams>>>>,
    paste_limit_bytes: Option<usize>,
}

impl MockProvider {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            launch_binary: None,
            supports_chat_ui: false,
            reasoning_efforts: Vec::new(),
            models: Vec::new(),
            system_prompt_delivery: SystemPromptDelivery::External,
            status_bar_rows: 0,
            update_command: None,
            directory_trust_response: None,
            classified_prompt_auto_response: None,
            tab_state_classification: None,
            parsed_prompt: None,
            parsed_directory_trust_prompt: None,
            sessions: Default::default(),
            session_file: None,
            session_models: Default::default(),
            session_reasoning_efforts: Default::default(),
            usage_source: None,
            empty_launch_command: false,
            empty_launch_command_when_task_present: false,
            task_delivery: TaskDelivery::PtyInput,
            session_starts_on_first_user_turn: false,
            provider_readiness_strategy: ProviderReadinessStrategy::default(),
            launch_artifact_sweep_log: None,
            launch_artifact_release_log: None,
            resume_context_log: None,
            paste_limit_bytes: None,
        }
    }

    pub fn with_launch_artifact_logs(
        mut self,
        sweep_log: Arc<Mutex<Vec<LaunchArtifactSweepRequest>>>,
        release_log: Arc<Mutex<Vec<(String, String)>>>,
    ) -> Self {
        self.launch_artifact_sweep_log = Some(sweep_log);
        self.launch_artifact_release_log = Some(release_log);
        self
    }

    pub fn with_resume_context_log(mut self, log: Arc<Mutex<Vec<AgentResumeParams>>>) -> Self {
        self.resume_context_log = Some(log);
        self
    }

    pub fn with_paste_limit_bytes(mut self, limit: usize) -> Self {
        self.paste_limit_bytes = Some(limit);
        self
    }

    /// Declares a usage log the provider owns for one session, so a test can drive
    /// the KPI ingest path through the capability instead of a hardcoded layout.
    pub fn with_usage_source(mut self, path: PathBuf, session_id: &str) -> Self {
        self.usage_source = Some(UsageSourceSpec {
            path,
            session_id: Some(session_id.to_string()),
        });
        self
    }

    /// Declares what the provider's own record says this exact session is running,
    /// so a test can prove a host path reads a session-scoped model instead of a
    /// provider-wide or launch-time guess.
    pub fn with_session_model(mut self, session_id: &str, model: &str) -> Self {
        self.session_models
            .insert(session_id.to_string(), model.to_string());
        self
    }

    pub fn with_session_reasoning_effort(mut self, session_id: &str, effort: &str) -> Self {
        self.session_reasoning_efforts
            .insert(session_id.to_string(), effort.to_string());
        self
    }

    /// Declares the provider's own session log, so a test can drive the host paths
    /// that read one through the capability instead of a hardcoded layout.
    pub fn with_session_file(id: &str, path: PathBuf) -> Self {
        Self {
            session_file: Some(path),
            ..Self::new(id)
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

    pub fn with_session_config_delivery(mut self) -> Self {
        self.system_prompt_delivery = SystemPromptDelivery::SessionConfig;
        self
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

    pub fn with_model_reasoning_efforts(id: &str, model: &str, levels: &[&str]) -> Self {
        let mut provider = Self::with_reasoning_efforts(id, &["low", "medium", "high", "xhigh"]);
        provider.models = vec![ModelInfo {
            default_reasoning_effort: None,
            id: model.to_string(),
            display_name: model.to_string(),
            reasoning_efforts: Some(
                levels
                    .iter()
                    .map(|level| ReasoningEffortInfo {
                        id: (*level).to_string(),
                        display_name: (*level).to_string(),
                        description: None,
                    })
                    .collect(),
            ),
            ..ModelInfo::default()
        }];
        provider
    }

    pub fn with_models(mut self, models: Vec<ModelInfo>) -> Self {
        self.models.extend(models);
        self
    }

    pub fn with_update_command(id: &str, command: &str) -> Self {
        Self {
            update_command: Some(command.to_string()),
            ..Self::new(id)
        }
    }

    pub fn with_directory_trust_response(
        id: &str,
        response: codeterm_plugin::provider::DirectoryTrustPromptResponse,
    ) -> Self {
        Self {
            directory_trust_response: Some(response),
            ..Self::new(id)
        }
    }

    pub fn with_tab_state(
        mut self,
        classification: codeterm_plugin::tab_state::TabStateClassification,
    ) -> Self {
        self.tab_state_classification = Some(classification);
        self
    }

    pub fn with_parsed_prompt(mut self, prompt: codeterm_types::pty::PendingPrompt) -> Self {
        self.parsed_prompt = Some(prompt);
        self
    }

    pub fn with_classified_prompt_auto_response(
        mut self,
        role: codeterm_plugin::provider::ClassifiedPromptOptionRole,
    ) -> Self {
        self.classified_prompt_auto_response = Some(role);
        self
    }

    pub fn with_directory_trust_prompt(
        mut self,
        prompt: codeterm_types::pty::PendingPrompt,
    ) -> Self {
        self.parsed_directory_trust_prompt = Some(prompt);
        self
    }

    pub fn with_empty_launch_command(id: &str) -> Self {
        Self {
            empty_launch_command: true,
            ..Self::new(id)
        }
    }

    pub fn with_empty_launch_command_when_task_present(id: &str) -> Self {
        Self {
            empty_launch_command_when_task_present: true,
            system_prompt_delivery: SystemPromptDelivery::SessionConfig,
            task_delivery: TaskDelivery::InlineFlag,
            ..Self::new(id)
        }
    }

    pub fn with_launch_binary(mut self, binary: &str) -> Self {
        self.launch_binary = Some(binary.to_string());
        self
    }

    pub fn with_session(id: &str, session_id: &str) -> Self {
        Self {
            sessions: [session_id.to_string()].into_iter().collect(),
            ..Self::new(id)
        }
    }

    pub fn with_session_starts_on_first_user_turn(mut self) -> Self {
        self.session_starts_on_first_user_turn = true;
        self
    }

    pub fn with_provider_readiness_strategy(mut self, strategy: ProviderReadinessStrategy) -> Self {
        self.provider_readiness_strategy = strategy;
        self
    }
}

impl AgentProvider for MockProvider {
    fn paste_limit_bytes(&self) -> Option<usize> {
        self.paste_limit_bytes
    }

    fn id(&self) -> &str {
        &self.id
    }
    fn display_name(&self) -> &str {
        &self.id
    }
    fn binary_name(&self) -> &str {
        self.launch_binary.as_deref().unwrap_or(&self.id)
    }
    fn build_launch_command(&self, params: &AgentLaunchParams) -> String {
        if self.empty_launch_command
            || (self.empty_launch_command_when_task_present && params.task.is_some())
        {
            return String::new();
        }
        let mut cmd = format!("{}-cli", self.id);
        if let Some(args) = &params.args {
            for a in args {
                cmd.push(' ');
                cmd.push_str(a);
            }
        }
        if let Some(marker) = &params.launch_marker {
            cmd.push_str(" --launch-marker ");
            cmd.push_str(marker);
        }
        cmd
    }
    fn release_tab_launch_artifacts(
        &self,
        tab_id: &str,
        instance_id: &str,
    ) -> Result<bool, String> {
        if let Some(log) = &self.launch_artifact_release_log {
            log.lock()
                .push((tab_id.to_string(), instance_id.to_string()));
            return Ok(true);
        }
        Ok(false)
    }
    fn sweep_launch_artifacts(
        &self,
        request: &LaunchArtifactSweepRequest,
    ) -> Result<Option<LaunchArtifactSweepReport>, String> {
        let Some(log) = &self.launch_artifact_sweep_log else {
            return Ok(None);
        };
        log.lock().push(request.clone());
        Ok(Some(LaunchArtifactSweepReport::default()))
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
    fn available_models(&self) -> Vec<ModelInfo> {
        self.models.clone()
    }
    fn reasoning_effort_launch_args(&self, effort: &str) -> Vec<String> {
        if effort.is_empty() {
            return Vec::new();
        }
        vec!["--effort".into(), effort.into()]
    }
    fn task_delivery(&self) -> TaskDelivery {
        self.task_delivery.clone()
    }
    fn session_starts_on_first_user_turn(&self) -> bool {
        self.session_starts_on_first_user_turn
    }
    fn provider_readiness_strategy(&self) -> ProviderReadinessStrategy {
        self.provider_readiness_strategy.clone()
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
        self.session_file.clone()
    }
    fn usage_sources(&self, cwd: &str, session_id: &str) -> Vec<UsageSourceSpec> {
        if let Some(source) = &self.usage_source {
            return vec![source.clone()];
        }
        usage_sources_from_session_file(self, cwd, session_id)
    }
    fn detect_session_model(&self, _cwd: &str, session_id: &str) -> Option<String> {
        self.session_models.get(session_id).cloned()
    }
    fn detect_session_reasoning_effort(&self, _cwd: &str, session_id: &str) -> Option<String> {
        self.session_reasoning_efforts.get(session_id).cloned()
    }
    fn session_exists(&self, _cwd: &str, session_id: &str) -> bool {
        self.sessions.contains(session_id)
    }
    fn build_resume_command(&self, session_id: &str, _skip_permissions: bool) -> Option<String> {
        Some(format!("{}-cli --resume {session_id}", self.id))
    }
    fn build_resume_command_with_context(&self, params: &AgentResumeParams) -> Option<String> {
        if let Some(log) = &self.resume_context_log {
            log.lock().push(params.clone());
        }
        let base = self.build_resume_command(&params.session_id, params.skip_permissions)?;
        Some(codeterm_plugin::provider::append_launch_args(
            base,
            &params.args,
        ))
    }
    fn detect_session_id(
        &self,
        _cwd: &str,
        _exclude: &[String],
        _max_age: Option<Duration>,
    ) -> Option<String> {
        None
    }
    fn directory_trust_prompt_response(
        &self,
        _screen: &str,
    ) -> Option<codeterm_plugin::provider::DirectoryTrustPromptResponse> {
        self.directory_trust_response.clone()
    }
    fn classify_tab_state(
        &self,
        _snapshot: &codeterm_plugin::tab_state::TabStateSnapshot,
    ) -> Option<codeterm_plugin::tab_state::TabStateClassification> {
        self.tab_state_classification.clone()
    }
    fn supports_classified_prompt_auto_response(&self) -> bool {
        self.classified_prompt_auto_response.is_some()
    }
    fn classified_prompt_auto_response(
        &self,
        state: codeterm_plugin::tab_state::TabState,
        _screen: &str,
    ) -> Option<codeterm_plugin::provider::ClassifiedPromptOptionRole> {
        (state == codeterm_plugin::tab_state::TabState::UpdatePrompt)
            .then_some(self.classified_prompt_auto_response)
            .flatten()
    }
    fn parse_prompt(&self, _screen: &str) -> Option<codeterm_types::pty::PendingPrompt> {
        self.parsed_prompt.clone()
    }
    fn parse_directory_trust_prompt(
        &self,
        _screen: &str,
    ) -> Option<codeterm_types::pty::PendingPrompt> {
        self.parsed_directory_trust_prompt.clone().or_else(|| {
            self.directory_trust_response
                .as_ref()
                .and_then(|_| self.parsed_prompt.clone())
        })
    }
    fn supports_directory_trust_prompt_parsing(&self) -> bool {
        self.parsed_directory_trust_prompt.is_some()
            || (self.directory_trust_response.is_some() && self.parsed_prompt.is_some())
    }
}
