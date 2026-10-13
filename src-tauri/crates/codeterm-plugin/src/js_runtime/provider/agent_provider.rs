use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::provider::{
    self, AgentLaunchParams, AgentProvider, BusySubmitStrategy, LaunchArtifactSweepReport,
    LaunchArtifactSweepRequest, ModelGroup, ModelGroupUsage, ModelInfo, ProviderReadinessStrategy,
    ProviderTrust, ProviderUpdateAvailability, ReadinessStrategy, ReasoningEffortInfo,
    ReplyRelayEvidence, SessionIdInjection, SessionMessage, SessionMetadata, SettingField,
    SystemPromptDelivery, TaskDelivery, UsageSourceSpec, VerifySubmitStrategy,
};
use codeterm_types::pty::PendingPrompt;

use super::super::{call_json, manifest_settings_sections, IntegrationKind, SerdeModelInfo};
use super::command_template::{launch_command_from_template, resume_command_from_template};
use super::wire::{
    launch_params_to_json, SerdeSessionMetadata, VmUsageParser, SESSION_PARSE_CHUNK_BYTES,
};
use super::{launch_artifacts, prompt_parse, JsProvider};

mod chat_read;
mod delivery;
mod directory_trust;
mod model_launch;
mod session_detection;
mod session_wire;

use session_wire::WireUsageSource;

#[async_trait::async_trait]
impl AgentProvider for JsProvider {
    fn agent_runtime(&self) -> Option<crate::agent_runtime::AgentRuntimeCapability> {
        self.manifest.capabilities.agent_runtime.clone()
    }

    fn authorize_agent_runtime(
        &self,
        authority: &crate::agent_runtime::LaunchAuthority,
    ) -> Result<PathBuf, super::super::manifest::RuntimeManifestError> {
        self.manifest.authorize_agent_runtime(authority)
    }

    async fn runtime_plan(
        &self,
        ctx: &crate::agent_runtime::RuntimeContext,
        op: &crate::agent_runtime::RuntimeOperation,
    ) -> Result<crate::agent_runtime::RuntimePlan, crate::agent_runtime::RuntimeFailure> {
        self.js_runtime_plan(ctx, op).await
    }

    async fn runtime_response(
        &self,
        ctx: &crate::agent_runtime::RuntimeContext,
        op: &crate::agent_runtime::RuntimeOperation,
        state: &Value,
        reply: &crate::agent_runtime::RpcReply,
    ) -> Result<crate::agent_runtime::RuntimeStep, crate::agent_runtime::RuntimeFailure> {
        self.js_runtime_response(ctx, op, state, reply).await
    }

    async fn runtime_reduce(
        &self,
        ctx: &crate::agent_runtime::RuntimeContext,
        state: &Value,
        frames: &[crate::agent_runtime::RuntimeFrame],
    ) -> Result<crate::agent_runtime::RuntimeReduction, crate::agent_runtime::RuntimeFailure> {
        self.js_runtime_reduce(ctx, state, frames).await
    }

    fn auth_status_probe(&self) -> Option<provider::AuthStatusProbe> {
        self.try_call("authStatusProbe", &[]).flatten()
    }
    fn tracks_background_work(&self) -> bool {
        self.try_call("tracksBackgroundWork", &[]).unwrap_or(false)
    }
    fn capability_probe(&self) -> Option<provider::CapabilityProbe> {
        self.manifest.spawn.capability_probe
    }
    fn fork_session_args(&self) -> Option<Vec<String>> {
        self.manifest.spawn.fork_session_args.clone()
    }
    fn parse_background_work(
        &self,
        record: &Value,
    ) -> Vec<codeterm_types::agent::background_work::BackgroundWorkObservation> {
        self.try_call("parseBackgroundWork", &[record.clone()])
            .unwrap_or_default()
    }

    fn reasoning_effort_launch_args_for_model(
        &self,
        model: Option<&str>,
        effort: &str,
    ) -> Vec<String> {
        self.js_reasoning_effort_launch_args_for_model(model, effort)
    }

    fn parse_auth_status(&self, record: &Value) -> Option<provider::AuthStatus> {
        self.try_call("parseAuthStatus", &[record.clone()])
            .flatten()
    }

    fn parse_session_observations(&self, record: &Value) -> Option<provider::SessionObservations> {
        self.try_call("parseSessionObservations", &[record.clone()])
            .flatten()
    }
    fn parse_context_usage(
        &self,
        record: &serde_json::Value,
    ) -> Option<codeterm_types::agent::context::TurnContext> {
        self.try_call("parseContextUsage", &[record.clone()])
            .flatten()
    }
    fn cli_compatibility(&self) -> Option<crate::provider_compat::CompatibilityManifest> {
        self.manifest.cli_compatibility.clone()
    }

    fn id(&self) -> &str {
        &self.manifest.id
    }

    fn model_provider_config(
        &self,
    ) -> Option<std::sync::Arc<dyn crate::model_providers::ModelProviderConfig>> {
        self.manifest.capabilities.model_provider_config.then(|| {
            std::sync::Arc::new(super::super::model_providers::JsModelProviderConfig::new(
                self.vm.clone(),
            )) as std::sync::Arc<dyn crate::model_providers::ModelProviderConfig>
        })
    }

    fn display_name(&self) -> &str {
        &self.manifest.display_name
    }

    fn binary_name(&self) -> &str {
        self.manifest
            .binary_name
            .as_deref()
            .unwrap_or(&self.manifest.id)
    }

    fn paste_limit_bytes(&self) -> Option<usize> {
        self.manifest.spawn.paste_limit_bytes
    }

    fn paste_capability(&self) -> Option<provider::PasteCapability> {
        self.manifest.spawn.paste.clone()
    }

    fn additional_writable_dir_flag(&self) -> Option<String> {
        self.manifest.commands.additional_writable_dir_flag.clone()
    }

    fn sandboxed_linux_launch_capability(
        &self,
    ) -> Option<provider::SandboxedLinuxLaunchCapability> {
        self.manifest.spawn.sandboxed_linux_launch.clone()
    }

    fn launch_permission_capability(&self) -> Option<provider::LaunchPermissionCapability> {
        self.manifest.spawn.launch_permissions.clone()
    }

    fn build_launch_command(&self, params: &AgentLaunchParams) -> String {
        if !self.has_js("buildLaunchCommand") {
            if let Some(command) = launch_command_from_template(&self.manifest, params) {
                return command;
            }
        }
        self.try_call::<String>("buildLaunchCommand", &[launch_params_to_json(params)])
            .unwrap_or_default()
    }

    fn release_tab_launch_artifacts(
        &self,
        tab_id: &str,
        instance_id: &str,
    ) -> Result<bool, String> {
        launch_artifacts::release(self, tab_id, instance_id)
    }

    fn folder_trust_answered(&self, tab_id: &str, instance_id: &str) -> Result<bool, String> {
        const METHOD: &str = "folderTrustAnswered";
        if self.has_js(METHOD) {
            return super::call_json_strict(&self.vm, METHOD, &[json!(tab_id), json!(instance_id)])
                .map_err(|error| {
                    format!("[plugin {}] {METHOD} failed: {error}", self.manifest.id)
                });
        }
        self.native()
            .map(|native| native.folder_trust_answered(tab_id, instance_id))
            .unwrap_or(Ok(false))
    }

    fn sweep_launch_artifacts(
        &self,
        request: &LaunchArtifactSweepRequest,
    ) -> Result<Option<LaunchArtifactSweepReport>, String> {
        launch_artifacts::sweep(self, request)
    }

    fn starter_prompt_text(&self, prompt: crate::provider::StarterPrompt) -> Option<String> {
        prompt
            .host_text()
            .map(str::to_owned)
            .or_else(|| self.try_call::<String>("starterPromptText", &[json!(prompt)]))
    }

    fn readiness_strategy(&self) -> ReadinessStrategy {
        (&self.manifest.spawn.readiness_strategy).into()
    }

    fn task_delivery(&self) -> TaskDelivery {
        self.manifest.spawn.task_delivery.into()
    }

    fn system_prompt_delivery(&self) -> SystemPromptDelivery {
        (&self.manifest.spawn.system_prompt_delivery).into()
    }

    fn provider_readiness_strategy(&self) -> ProviderReadinessStrategy {
        (&self.manifest.spawn.provider_readiness).into()
    }

    fn composer_cursor_position(&self) -> crate::provider::ComposerCursorPosition {
        self.manifest.spawn.composer_cursor_position
    }

    fn input_fallback_ms(&self) -> Option<u64> {
        self.manifest.spawn.input_fallback_ms
    }

    fn onboarding_auto_answer(&self) -> provider::OnboardingAutoAnswer {
        self.manifest.spawn.onboarding_auto_answer
    }

    fn restores_conversation_history_on_resume(&self) -> bool {
        self.manifest.spawn.restores_conversation_history_on_resume
    }

    fn session_starts_on_first_user_turn(&self) -> bool {
        self.manifest.spawn.session_starts_on_first_user_turn
            || self
                .native()
                .is_some_and(|native| native.session_starts_on_first_user_turn())
    }

    fn public_send_waits_for_startup_turn_completion(&self) -> bool {
        self.manifest
            .spawn
            .public_send_waits_for_startup_turn_completion
            || self
                .native()
                .is_some_and(|native| native.public_send_waits_for_startup_turn_completion())
    }

    fn verify_submit_strategy(&self) -> VerifySubmitStrategy {
        (&self.manifest.spawn.verify_submit).into()
    }

    fn supports_unbound_submit_recovery(&self) -> bool {
        delivery::supports_unbound_submit_recovery(self)
    }

    fn turn_evidence_normalization(&self) -> crate::provider::TurnEvidenceNormalization {
        self.manifest.spawn.turn_evidence_normalization
    }

    fn busy_submit_strategy(&self) -> BusySubmitStrategy {
        (&self.manifest.spawn.busy_submit).into()
    }

    fn detect_from_title(&self, title: &str) -> bool {
        self.try_call::<bool>("detectFromTitle", &[json!(title)])
            .unwrap_or(false)
    }

    fn detect_from_output(&self, text: &str) -> bool {
        self.try_call::<bool>("detectFromOutput", &[json!(text)])
            .unwrap_or(false)
    }

    fn screen_has_tui(&self, screen_text: &str) -> bool {
        self.try_call::<bool>("screenHasTui", &[json!(screen_text)])
            .unwrap_or(false)
    }

    fn extract_session_name(&self, screen: &str) -> Option<String> {
        self.try_call::<Option<String>>("extractSessionName", &[json!(screen)])
            .flatten()
    }

    fn classify_tab_state(
        &self,
        snapshot: &crate::tab_state::TabStateSnapshot,
    ) -> Option<crate::tab_state::TabStateClassification> {
        if !self.has_js("classifyTabState") {
            return self.native().and_then(|n| n.classify_tab_state(snapshot));
        }
        let arg = serde_json::to_value(snapshot).ok()?;
        self.try_call::<Option<crate::tab_state::TabStateClassification>>(
            "classifyTabState",
            &[arg],
        )
        .flatten()
    }

    fn delivery_modal_visible(&self, snapshot: &crate::tab_state::TabStateSnapshot) -> bool {
        delivery::delivery_modal_visible(self, snapshot)
    }

    fn is_output_noise(&self, text: &str) -> bool {
        self.try_call::<bool>("isOutputNoise", &[json!(text)])
            .unwrap_or(false)
    }

    fn status_bar_rows(&self) -> u16 {
        self.manifest.spawn.status_bar_rows
    }

    fn supports_agent_state_status_reconciliation(&self) -> bool {
        self.manifest.spawn.agent_state_status_reconciliation
    }

    fn has_event_markers(&self, chunk: &str) -> bool {
        // Default: arrows / proceed / 1./2. heuristic from trait, but let the plugin override when it has its own fingerprints.
        if let Some(v) = self.try_call::<bool>("hasEventMarkers", &[json!(chunk)]) {
            return v;
        }
        chunk.contains("❯")
            || chunk.contains("proceed")
            || chunk.contains("Esc to cancel")
            || chunk.contains("Tab to amend")
            || (chunk.contains("1.") && chunk.contains("2."))
    }

    fn detect_events(&self, text: &str) -> Vec<crate::detector::AgentEvent> {
        // universal detector — anything else returned by the plugin is ignored until the AgentEvent enum grows new variants.
        let names: Vec<String> = self
            .try_call::<Vec<String>>("detectEvents", &[json!(text)])
            .unwrap_or_default();
        let mut out = Vec::new();
        for n in names {
            if n == "permission_request" {
                out.push(crate::detector::AgentEvent::PermissionRequest);
            } else if n == "turn_error" {
                out.push(crate::detector::AgentEvent::TurnError);
            } else if n == "task_complete" {
                out.push(crate::detector::AgentEvent::TaskComplete);
            } else if n == "turn_started" {
                out.push(crate::detector::AgentEvent::TurnStarted);
            }
        }
        if out.is_empty() && crate::detector::detect_interactive_prompt(text) {
            out.push(crate::detector::AgentEvent::PermissionRequest);
        }
        out
    }

    fn update_prompt_safe_choice(&self, prompt: &PendingPrompt) -> Option<String> {
        self.update_prompt_safe_choice_from_js(prompt)
    }

    fn parse_prompt(&self, text: &str) -> Option<PendingPrompt> {
        if let Some(v) = self.try_call::<Option<Value>>("parsePrompt", &[json!(text)]) {
            return v.and_then(prompt_parse::parsed_prompt_from_js);
        }
        crate::detector::parse_permission_prompt(text)
            .map(|(question, options)| crate::prompt::build_prompt(question, options))
    }

    fn parse_directory_trust_prompt(&self, screen: &str) -> Option<PendingPrompt> {
        if let Some(value) =
            self.try_call::<Option<Value>>("parseDirectoryTrustPrompt", &[json!(screen)])
        {
            return value.and_then(prompt_parse::parsed_prompt_from_js);
        }
        self.native()
            .and_then(|provider| provider.parse_directory_trust_prompt(screen))
    }

    fn build_prompt_response(&self, option_index: usize, screen: &str) -> Vec<Vec<u8>> {
        // Plugin returns a list of base64-encoded byte strings, one per PTY write step. Lets plugins emit vim j/k, arrow keys, escape sequences — whatever the TUI expects. `screen` lets a plugin navigate relative to the live highlight (cursor /model).
        if let Some(steps) = self
            .try_call::<Vec<String>>("buildPromptResponse", &[json!(option_index), json!(screen)])
        {
            use base64::Engine;
            let engine = base64::engine::general_purpose::STANDARD;
            return steps
                .into_iter()
                .filter_map(|s| engine.decode(s.as_bytes()).ok())
                .collect();
        }
        let mut steps = Vec::new();
        let arrow_down = b"\x1b[B".to_vec();
        for _ in 0..option_index {
            steps.push(arrow_down.clone());
        }
        steps.push(b"\r".to_vec());
        steps
    }

    fn directory_trust_prompt_response(
        &self,
        screen: &str,
    ) -> Option<crate::provider::DirectoryTrustPromptResponse> {
        self.js_directory_trust_prompt_response(screen)
    }

    fn pre_trust_directory(&self, path: &str) -> Result<crate::provider::PreTrustOutcome, String> {
        self.js_pre_trust_directory(path)
    }

    fn is_directory_trusted(&self, path: &str) -> Result<bool, String> {
        self.js_is_directory_trusted(path)
    }

    fn directory_trust_state(
        &self,
        path: &str,
        skip_permissions: bool,
    ) -> Option<crate::provider::DirectoryTrustState> {
        self.js_directory_trust_state(path, skip_permissions)
    }

    fn launch_onboarding_response(
        &self,
        screen: &str,
    ) -> Option<crate::provider::ProviderPromptKeyStep> {
        self.launch_onboarding_response_from_js(screen)
    }

    fn launch_onboarding_safe_choice(&self, screen: &str) -> Option<String> {
        self.try_call::<Option<String>>("launchOnboardingSafeChoice", &[json!(screen)])?
    }

    fn live_prompt_response(&self, option_index: usize, screen: &str) -> Option<Vec<Vec<u8>>> {
        self.live_prompt_response_from_js(option_index, screen)
    }

    fn permission_prompt_response(
        &self,
        tool_name: &str,
        prompt: &PendingPrompt,
        screen: &str,
    ) -> Option<crate::permission_prompt::PermissionPromptResponse> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct WireResponse {
            option_index: usize,
            rule: String,
            steps: Vec<String>,
        }
        let value = serde_json::to_value(prompt).ok()?;
        let wire = self.try_call::<Option<WireResponse>>(
            "permissionPromptResponse",
            &[json!(tool_name), value, json!(screen)],
        )??;
        use base64::Engine;
        let engine = base64::engine::general_purpose::STANDARD;
        let steps = wire
            .steps
            .iter()
            .map(|step| engine.decode(step.as_bytes()).ok())
            .collect::<Option<Vec<_>>>()?;
        let rule = wire.rule.trim();
        (!steps.is_empty() && !rule.is_empty()).then(|| {
            crate::permission_prompt::PermissionPromptResponse {
                option_index: wire.option_index,
                rule: rule.to_string(),
                steps,
            }
        })
    }

    fn session_model_identity_path(&self, cwd: &str, session_id: &str) -> Option<PathBuf> {
        self.owned_session_model_identity_path(cwd, session_id)
    }

    fn session_file_path(&self, cwd: &str, session_id: &str) -> Option<PathBuf> {
        // Hook-reported evidence wins over any reconstruction; absence falls through.
        let path = crate::session_evidence::stored_transcript_path(session_id)
            .or_else(|| self.owned_session_file_path(cwd, session_id))?;
        if self
            .manifest
            .commands
            .model_switch_commands
            .as_ref()
            .is_some_and(|commands| commands.turn_completion.is_some())
        {
            self.permitted_session_path(path)
        } else {
            Some(path)
        }
    }

    fn additional_usage_sources(&self, session_file: &Path, session_id: &str) -> Vec<PathBuf> {
        if !self.has_js("sessionUsageSources") {
            return self
                .native()
                .map(|n| n.additional_usage_sources(session_file, session_id))
                .unwrap_or_default();
        }
        self.try_call::<Vec<String>>(
            "sessionUsageSources",
            &[json!(session_file.to_string_lossy()), json!(session_id)],
        )
        .unwrap_or_default()
        .into_iter()
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .collect()
    }

    fn usage_sources(&self, cwd: &str, session_id: &str) -> Vec<UsageSourceSpec> {
        if self.has_js("usageSources") {
            // An exported `usageSources` is authoritative: a null or failed reply
            // declares no source and must not fall back to a fabricated file.
            return self
                .try_call::<Vec<WireUsageSource>>("usageSources", &[json!(cwd), json!(session_id)])
                .unwrap_or_default()
                .into_iter()
                .filter(|source| !source.path.is_empty())
                .map(|source| UsageSourceSpec {
                    path: PathBuf::from(source.path),
                    session_id: source.session_id.filter(|id| !id.is_empty()),
                })
                .collect();
        }
        provider::usage_sources_from_session_file(self, cwd, session_id)
    }

    fn session_exists(&self, cwd: &str, session_id: &str) -> Option<bool> {
        if self.has_js("sessionExists") {
            return self
                .try_call::<Option<bool>>("sessionExists", &[json!(cwd), json!(session_id)])
                .flatten();
        }
        self.owned_session_file_path(cwd, session_id)?
            .try_exists()
            .ok()
    }

    fn detect_session_model(&self, cwd: &str, session_id: &str) -> Option<String> {
        self.detect_session_model_wire("detectSessionModel", &[json!(cwd), json!(session_id)])
    }

    fn session_model_transcript_boundary(
        &self,
        cwd: &str,
        session_id: &str,
    ) -> Option<codeterm_types::agent::SessionModelTranscriptBoundary> {
        self.try_call::<Option<codeterm_types::agent::SessionModelTranscriptBoundary>>(
            "sessionModelTranscriptBoundary",
            &[json!(cwd), json!(session_id)],
        )
        .flatten()
    }

    fn detect_session_model_after_boundary(
        &self,
        cwd: &str,
        session_id: &str,
        boundary: &codeterm_types::agent::SessionModelTranscriptBoundary,
    ) -> Option<String> {
        self.detect_session_model_wire(
            "detectSessionModelAfterBoundary",
            &[json!(cwd), json!(session_id), json!(boundary)],
        )
    }

    fn detect_session_reasoning_effort(&self, cwd: &str, session_id: &str) -> Option<String> {
        self.try_call::<Option<String>>(
            "detectSessionReasoningEffort",
            &[json!(self.session_cwd(cwd)), json!(session_id)],
        )
        .flatten()
    }

    fn session_created_at(&self, cwd: &str, session_id: &str) -> Option<std::time::SystemTime> {
        let ms = self
            .try_call::<Option<i64>>("sessionCreatedMs", &[json!(cwd), json!(session_id)])
            .flatten()?;
        if ms < 0 {
            return None;
        }
        Some(std::time::UNIX_EPOCH + std::time::Duration::from_millis(ms as u64))
    }

    fn detect_active_session(&self, cwd: &str, current_sid: &str) -> Option<String> {
        self.try_call::<Option<String>>("detectActiveSession", &[json!(cwd), json!(current_sid)])
            .flatten()
    }

    fn session_id_injection(&self) -> SessionIdInjection {
        self.manifest.spawn.session_id_injection.into()
    }

    fn context_attach(&self) -> Option<crate::provider::ContextAttach> {
        self.manifest.capabilities.context_attach
    }

    fn is_tool_less(&self) -> bool {
        self.manifest.capabilities.tool_less
    }

    fn supports_reply_relay(&self) -> bool {
        self.has_reply_relay
    }

    fn read_reply_relay_turn(
        &self,
        cwd: &str,
        sid: &str,
        after: u64,
    ) -> Option<ReplyRelayEvidence> {
        super::reply_relay::read(self, cwd, sid, after)
    }

    fn supports_launch_session_detection(&self) -> bool {
        self.has_launch_session_detection
    }

    fn detect_launch_session(
        &self,
        evidence: &provider::LaunchSessionEvidence,
    ) -> Option<provider::LaunchSessionDetection> {
        self.try_call::<Option<provider::LaunchSessionDetection>>(
            "detectLaunchSession",
            &[json!(evidence)],
        )
        .flatten()
    }

    fn detect_session_id(
        &self,
        cwd: &str,
        exclude: &[String],
        max_age: Option<Duration>,
    ) -> Option<String> {
        // manifest.sessions.* when the plugin doesn't implement `detectSessionId`.
        let max_age_ms = max_age.map(|d| d.as_millis() as i64).unwrap_or(300_000);
        if let Some(v) = self.try_call::<Option<String>>(
            "detectSessionId",
            &[json!(cwd), json!(exclude), json!(max_age_ms)],
        ) {
            return v;
        }
        let dir = self.sessions_dir(cwd)?;
        session_detection::unique_recent_session(
            &dir,
            &self.manifest.sessions.as_ref()?.file_extension,
            exclude,
            max_age,
        )
    }

    fn enumerate_sessions(&self) -> Vec<SessionMetadata> {
        if let Some(list) = self.try_call::<Vec<SerdeSessionMetadata>>("enumerateSessions", &[]) {
            return list
                .into_iter()
                .map(|m| m.into_session_metadata(&self.manifest.id))
                .collect();
        }
        self.manifest_enumerate_sessions()
    }

    fn enumerate_sessions_for_search(&self) -> Vec<SessionMetadata> {
        if self.session_content_source() == provider::SessionContentSource::SessionFile
            && self.manifest.sessions.is_some()
            && (self
                .manifest
                .sessions
                .as_ref()
                .is_some_and(|sessions| sessions.search_files.is_some())
                || !self.has_js("enumerateSessions"))
        {
            return self.manifest_enumerate_sessions_for_search();
        }
        self.enumerate_sessions()
    }

    fn describe_session_for_search(&self, session: &SessionMetadata) -> SessionMetadata {
        self.js_describe_session_for_search(session)
    }

    fn find_session(&self, session_id: &str) -> Option<SessionMetadata> {
        if self.has_js("findSession") {
            return self
                .try_call::<Option<SerdeSessionMetadata>>("findSession", &[json!(session_id)])
                .flatten()
                .map(|m| m.into_session_metadata(&self.manifest.id))
                .filter(|m| m.session_id == session_id);
        }
        if self.manifest.sessions.is_some() {
            return self.manifest_find_session(session_id);
        }
        self.native().and_then(|n| n.find_session(session_id))
    }

    fn parse_session_messages(&self, file_path: &Path) -> Vec<SessionMessage> {
        if !self.has_js("parseSessionMessages") {
            return match self.native() {
                Some(n) => n.parse_session_messages(file_path),
                None => Vec::new(),
            };
        }
        let Ok(file) = std::fs::File::open(file_path) else {
            return Vec::new();
        };
        let mut reader = BufReader::new(file);
        let mut out = Vec::new();
        let mut chunk = String::new();
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {}
                Err(_) => break,
            }
            chunk.push_str(&line);
            if chunk.len() >= SESSION_PARSE_CHUNK_BYTES {
                self.extend_parsed_session_chunk(&mut out, &chunk);
                chunk.clear();
            }
        }
        if !chunk.is_empty() {
            self.extend_parsed_session_chunk(&mut out, &chunk);
        }
        out
    }

    fn session_content_source(&self) -> provider::SessionContentSource {
        self.js_session_content_source()
    }

    fn read_session_content(
        &self,
        session_id: &str,
        query: &provider::SessionContentQuery,
    ) -> Option<provider::SessionContent> {
        self.js_read_session_content(session_id, query)
    }

    fn session_jsonl_path(&self, cwd: &str, session_id: &str) -> Option<std::path::PathBuf> {
        self.js_session_jsonl_path(cwd, session_id)
    }

    fn structured_tool_file_diffs(&self, part: &serde_json::Value) -> Option<serde_json::Value> {
        self.js_structured_tool_file_diffs(part)
    }

    fn uses_structured_chat(&self) -> bool {
        if self.has_js("usesStructuredChat") {
            return self.try_call("usesStructuredChat", &[]).unwrap_or(false);
        }
        self.native()
            .map(|n| n.uses_structured_chat())
            .unwrap_or(false)
    }

    fn read_structured_chat(
        &self,
        cwd: &str,
        session_id: &str,
        cursor: Option<&str>,
    ) -> codeterm_types::agent::chat_types::StructuredChat {
        self.js_read_structured_chat(cwd, session_id, cursor)
    }

    fn supports_chat_ui(&self) -> bool {
        // structured-capable, or a native fallback that serves chat.
        self.manifest.capabilities.agent_runtime.is_some()
            || self.has_js("sessionJsonlPath")
            || self.uses_structured_chat()
            || self.native().map(|n| n.supports_chat_ui()).unwrap_or(false)
    }

    fn parse_session_delta(
        &self,
        path: &std::path::Path,
        from_offset: u64,
    ) -> codeterm_types::agent::chat_types::SessionDelta {
        self.js_parse_session_delta(path, from_offset)
    }

    fn parse_interactive_events(
        &self,
        msg: &codeterm_types::agent::chat_types::RichMessage,
    ) -> Vec<codeterm_types::agent::chat_types::InteractiveElement> {
        self.js_parse_interactive_events(msg)
    }

    fn build_resume_command(&self, session_id: &str, skip_permissions: bool) -> Option<String> {
        if !self.has_js("buildResumeCommand") {
            if let Some(command) =
                resume_command_from_template(&self.manifest, session_id, skip_permissions)
            {
                return Some(command);
            }
        }
        self.try_call::<Option<String>>(
            "buildResumeCommand",
            &[json!(session_id), json!(skip_permissions)],
        )
        .flatten()
    }

    fn build_resume_command_with_context(
        &self,
        params: &crate::provider::AgentResumeParams,
    ) -> Option<String> {
        // A missing export is the undeclared class, not a failure: fall back to
        // the plain resume so the prompt is never re-applied by accident.
        match self.try_call::<Option<String>>("buildResumeCommandWithContext", &[json!(params)]) {
            Some(command) => command,
            None => self
                .build_resume_command(&params.session_id, params.skip_permissions)
                .map(|base| crate::provider::append_launch_args(base, &params.args)),
        }
    }

    fn has_hooks(&self) -> bool {
        self.manifest.spawn.has_hooks
    }

    fn hook_event_roster(&self) -> Vec<provider::HookEventBinding> {
        match self.declared_hook_event_roster() {
            Some(Ok(roster)) => roster,
            Some(Err(error)) => {
                log::warn!(
                    "[plugin:{}] {error}; registering no hook routes",
                    self.manifest.id
                );
                Vec::new()
            }
            None => match self.native() {
                Some(native) => native.hook_event_roster(),
                None if self.manifest.spawn.has_hooks => provider::session_start_hook_roster(),
                None => Vec::new(),
            },
        }
    }

    fn supports_permission_auto_confirm(&self) -> bool {
        self.has_js("permissionPromptResponse")
    }

    fn supports_classified_prompt_auto_response(&self) -> bool {
        self.has_js("classifiedPromptAutoResponse")
            || self
                .native()
                .is_some_and(|native| native.supports_classified_prompt_auto_response())
    }

    fn classified_prompt_auto_response(
        &self,
        state: crate::tab_state::TabState,
        screen: &str,
    ) -> Option<crate::provider::ClassifiedPromptOptionRole> {
        if self.has_js("classifiedPromptAutoResponse") {
            self.try_call::<Option<crate::provider::ClassifiedPromptOptionRole>>(
                "classifiedPromptAutoResponse",
                &[json!(state), json!(screen)],
            )
            .flatten()
        } else {
            self.native()
                .and_then(|native| native.classified_prompt_auto_response(state, screen))
        }
    }

    fn structured_control(&self) -> Option<crate::provider::StructuredControl> {
        self.manifest.capabilities.structured_control.clone()
    }

    fn supports_prompt_answers(&self) -> bool {
        self.manifest
            .spawn
            .supports_prompt_answers
            .unwrap_or_else(|| self.native().is_none_or(|n| n.supports_prompt_answers()))
    }

    fn supports_structured_prompts(&self) -> bool {
        self.manifest.spawn.supports_structured_prompts
            || self
                .native()
                .is_some_and(|n| n.supports_structured_prompts())
    }

    fn supports_directory_trust_prompt_parsing(&self) -> bool {
        self.manifest.spawn.supports_directory_trust_prompt_parsing
            || self
                .native()
                .is_some_and(|n| n.supports_directory_trust_prompt_parsing())
    }

    fn structured_pending_prompt(&self, cwd: &str, session_id: &str) -> Option<PendingPrompt> {
        self.native()
            .and_then(|n| n.structured_pending_prompt(cwd, session_id))
    }

    fn needs_structured_prompt_server(&self) -> bool {
        self.manifest.spawn.structured_prompt_server
            || self
                .native()
                .is_some_and(|n| n.needs_structured_prompt_server())
    }

    fn auto_approve_flags(&self) -> Vec<String> {
        self.manifest.commands.auto_approve_flags.clone()
    }

    fn model_launch_args(&self, model: &str) -> Vec<String> {
        self.js_model_launch_args(model)
    }

    fn reasoning_efforts(&self) -> Vec<ReasoningEffortInfo> {
        crate::model_catalog::resolve_reasoning_efforts(&self.manifest.id)
    }

    fn default_reasoning_effort(&self) -> Option<String> {
        crate::model_catalog::resolve_default_reasoning_effort(&self.manifest.id)
            .or_else(|| self.manifest.default_reasoning_effort.clone())
    }

    fn reasoning_effort_launch_args(&self, effort: &str) -> Vec<String> {
        self.js_reasoning_effort_launch_args(effort)
    }

    fn compact_command(&self) -> Option<&str> {
        self.manifest.commands.compact.as_deref()
    }

    fn fresh_context_command(&self) -> Option<&str> {
        self.manifest.commands.fresh_context.as_deref()
    }

    fn exit_command(&self) -> Option<&str> {
        self.manifest.commands.exit.as_deref()
    }

    fn supports_idle_restart_termination(&self) -> bool {
        self.manifest.commands.idle_restart_termination
    }

    fn stop_keystroke(&self) -> Option<&str> {
        self.manifest.commands.stop_key.as_deref()
    }

    fn uses_win32_control_keys(&self) -> bool {
        self.manifest.commands.uses_win32_control_keys
    }

    fn stop_keys(&self) -> Option<Vec<String>> {
        if let Some(keys) = self.manifest.commands.stop_keys.as_ref() {
            return (!keys.is_empty() && keys.iter().all(|key| !key.is_empty()))
                .then(|| keys.clone());
        }
        self.stop_keystroke().map(|key| vec![key.to_string()])
    }

    fn stop_key_wait_for_output(&self) -> bool {
        self.manifest.commands.stop_key_wait_for_output
    }

    fn trust(&self) -> ProviderTrust {
        self.trust
    }

    fn is_native(&self) -> bool {
        false
    }

    fn usage_probe_command(&self) -> Option<&str> {
        self.manifest.spawn.usage_probe_command.as_deref()
    }

    fn usage_parser(&self) -> Option<&dyn crate::usage_parser::UsageParser> {
        self.usage_parser_cell
            .get_or_init(|| {
                self.has_js("parseUsage").then(|| VmUsageParser {
                    vm: self.vm.clone(),
                    plugin_id: self.manifest.id.clone(),
                })
            })
            .as_ref()
            .map(|p| p as &dyn crate::usage_parser::UsageParser)
    }

    fn usage_is_fetch_based(&self) -> bool {
        self.has_js("fetchUsage")
    }

    fn fetch_usage(&self) -> Option<Result<String, String>> {
        if !self.has_js("fetchUsage") {
            return None;
        }
        let now_ms = chrono::Utc::now().timestamp_millis();
        let denials_before = self.vm.capability_denial_count();
        let outcome = match call_json::<Option<String>>(&self.vm, "fetchUsage", &[json!(now_ms)]) {
            Ok(Some(raw)) => Ok(raw),
            Ok(None) => Err("fetchUsage returned null".to_string()),
            Err(e) => Err(format!(
                "[plugin {}] fetchUsage failed: {e}",
                self.manifest.id
            )),
        };
        Some(crate::provider::attribute_capability_denial(
            outcome,
            denials_before,
            self.vm.capability_denial_count(),
        ))
    }

    fn model_switch(&self) -> provider::ModelSwitchMode {
        self.manifest.commands.model_switch
    }

    fn model_switch_commands(&self) -> Option<provider::ModelSwitchCommands> {
        self.manifest.commands.model_switch_commands.clone()
    }

    fn supports_lazy_skills(&self) -> bool {
        self.manifest.spawn.supports_lazy_skills
    }

    fn icon_hint(&self) -> Option<&str> {
        None
    }

    fn icon_svg(&self) -> Option<&str> {
        self.manifest.icon_svg.as_deref()
    }

    fn icon_color(&self) -> Option<&str> {
        self.manifest.icon_color.as_deref()
    }

    fn cli_binary(&self) -> Option<&str> {
        self.manifest.cli_binary.as_deref()
    }

    fn install_command(&self, platform: &str) -> Option<String> {
        self.manifest.installer.get(platform).cloned()
    }

    fn badge(&self) -> Option<&str> {
        self.manifest.badge.as_deref()
    }

    fn update_command(&self, platform: &str) -> Option<String> {
        self.manifest.updater.get(platform).cloned()
    }

    fn update_prompt_availability(&self, screen: &str) -> Option<ProviderUpdateAvailability> {
        self.update_prompt_availability_from_js(screen)
    }

    fn supports_update_check(&self) -> bool {
        self.has_js("checkForUpdate")
    }

    fn check_for_update(&self) -> Option<ProviderUpdateAvailability> {
        // A plugin reports "no update" as null; only a malformed answer is an error.
        self.try_call::<Option<ProviderUpdateAvailability>>("checkForUpdate", &[])
            .flatten()
    }

    fn available_models(&self) -> Vec<ModelInfo> {
        crate::model_catalog::resolve_models(&self.manifest.id)
    }

    fn model_catalogue_capability(&self) -> provider::ModelCatalogueCapability {
        self.manifest.model_catalogue.clone()
    }

    async fn discover_models(&self) -> Option<Vec<ModelInfo>> {
        // `null` is the plugin's honest "no genuine discovery" signal — it must
        // not deserialize-fail into warn noise, and never falls back to a seed.
        self.try_call::<Option<Vec<SerdeModelInfo>>>("discoverModels", &[])
            .flatten()
            .map(|models| models.iter().map(ModelInfo::from).collect())
    }

    fn default_model(&self) -> Option<String> {
        crate::model_catalog::resolve_default(&self.manifest.id).or_else(|| {
            (!self.manifest.model_catalogue.live_discovery_required)
                .then(|| self.manifest.default_model.clone())
                .flatten()
        })
    }

    fn model_groups(&self) -> Vec<ModelGroup> {
        self.manifest.model_groups.clone()
    }

    fn model_group_usage(&self) -> Vec<ModelGroupUsage> {
        self.try_call::<Vec<ModelGroupUsage>>("modelGroupUsage", &[])
            .unwrap_or_default()
    }

    fn reports_group_usage(&self) -> bool {
        self.has_js("modelGroupUsage")
    }

    fn model_credential_files(&self) -> Vec<PathBuf> {
        self.manifest
            .model_catalogue
            .fingerprint_files(super::host_credentials::declared_files(
                &self.manifest.credentials,
            ))
    }

    fn settings_schema(&self) -> Vec<SettingField> {
        if !self.has_custom_settings_schema {
            let mut fields = manifest_settings_sections(&self.manifest);
            if self.auth_status_probe().is_some() {
                fields.push(provider::structured_signals_section());
            }
            return fields;
        }
        // Cached: describe() polls hit this; the plugin schema is a pure constant.
        self.schema_cache
            .get_or_init(|| self.compute_schemas())
            .0
            .clone()
    }

    fn notifications_schema(&self) -> Vec<SettingField> {
        self.schema_cache
            .get_or_init(|| self.compute_schemas())
            .1
            .clone()
    }

    fn install_bridge_rule(&self, payload: &str) -> Result<(), String> {
        self.install_bridge_rule_from_manifest(payload)
    }

    fn uninstall_bridge_rule(&self) -> Result<(), String> {
        self.uninstall_bridge_rule_from_manifest()
    }

    fn check_integration(&self) -> serde_json::Value {
        self.js_check_integration()
    }
    fn integration_kind(&self) -> IntegrationKind {
        self.js_integration_kind()
    }
    fn permission_hook_capability(&self) -> Option<provider::PermissionHookCapability> {
        self.js_permission_hook_capability()
    }
    fn install_integration(
        &self,
        api_port: u16,
        options: &crate::provider::IntegrationInstallOptions,
    ) -> Result<(), String> {
        self.js_install_integration(api_port, options)
    }
    fn uninstall_integration(&self) -> Result<(), String> {
        self.js_uninstall_integration()
    }
    fn restore_integration(&self) -> Result<(), String> {
        self.js_restore_integration()
    }
    fn integration_trust_grant_capability(
        &self,
    ) -> Option<crate::provider::IntegrationTrustGrantCapability> {
        self.js_integration_trust_grant_capability()
    }
    fn grant_integration_trust(&self, managed_keys: &[String]) -> serde_json::Value {
        self.js_grant_integration_trust(managed_keys)
    }
    fn ensure_integration_on_startup(&self, api_port: u16) {
        self.js_ensure_integration_on_startup(api_port)
    }
}
