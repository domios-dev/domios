use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};

mod command_template;
mod launch_artifacts;
mod manifest_sessions;
mod prompt_parse;
mod wire;

use command_template::{launch_command_from_template, resume_command_from_template};
use wire::{
    launch_params_to_json, result_from_js, SerdeSessionMessage, SerdeSessionMetadata,
    VmUsageParser, SESSION_PARSE_CHUNK_BYTES,
};

use crate::provider::{
    self, AgentLaunchParams, AgentProvider, BusySubmitStrategy, LaunchArtifactSweepReport,
    LaunchArtifactSweepRequest, ModelGroup, ModelGroupUsage, ModelInfo, ProviderReadinessStrategy,
    ProviderTrust, ReadinessStrategy, ReasoningEffortInfo, SessionIdInjection, SessionMessage,
    SessionMetadata, SettingField, SystemPromptDelivery, TaskDelivery, UsageSourceSpec,
    VerifySubmitStrategy,
};

use super::{
    call_json, call_json_strict, manifest_settings_sections, IntegrationKind, PluginManifest,
    PluginVm, SerdeModelInfo, VmError,
};
use codeterm_types::pty::PendingPrompt;

/// One generic provider per plugin bundle.
pub struct JsProvider {
    vm: Arc<PluginVm>,
    manifest: PluginManifest,
    trust: ProviderTrust,
    /// Registry handle for lazy native-fallback lookup. Capability methods the
    /// JS plugin does NOT export delegate to the native provider of the same id
    /// (displaced when this JS plugin took authority) — so a stub JS plugin
    /// (e.g. opencode, no sqlite in JS) still gets the native implementation.
    /// Looked up lazily so it survives hot-reload re-registration. Generic: any
    /// provider source (json/sql/…) works as long as something implements the
    /// trait method.
    registry: Option<Arc<dyn crate::host_traits::PluginProviderRegistrar>>,
    /// settingsSchema/notificationsSchema are pure constants in the plugin but
    /// `describe()` (called on every /api/agents/providers poll) re-evaluated
    /// them through QuickJS every time — a profiled CPU hot path. Compute once.
    schema_cache: std::sync::OnceLock<(Vec<SettingField>, Vec<SettingField>)>,
    has_custom_settings_schema: bool,
    /// VM-backed usage parser, built once when the plugin exports `parseUsage`.
    /// `usage_parser()` hands out a reference, so the adapter must live on the
    /// provider.
    usage_parser_cell: std::sync::OnceLock<Option<VmUsageParser>>,
}

impl JsProvider {
    pub fn new(manifest: PluginManifest, vm: Arc<PluginVm>) -> Self {
        Self::new_with_trust(manifest, vm, ProviderTrust::ThirdParty)
    }

    pub fn new_with_trust(
        manifest: PluginManifest,
        vm: Arc<PluginVm>,
        trust: ProviderTrust,
    ) -> Self {
        // Register the plugin's models with the catalog so settings UI and `default_model()` resolve through the same plumbing as native providers.
        if !manifest.models.is_empty() {
            crate::model_catalog::register(
                &manifest.id,
                crate::model_catalog::RegisteredDefaults {
                    models: manifest.models.iter().map(ModelInfo::from).collect(),
                    default: manifest.default_model.clone(),
                },
            );
        }
        crate::model_catalog::register_reasoning_efforts(
            &manifest.id,
            manifest
                .reasoning_efforts
                .iter()
                .map(ReasoningEffortInfo::from)
                .collect(),
            manifest.default_reasoning_effort.clone(),
        );
        let has_custom_settings_schema = super::vm::has_method(&vm, "settingsSchema");
        Self {
            vm,
            manifest,
            trust,
            registry: None,
            schema_cache: std::sync::OnceLock::new(),
            has_custom_settings_schema,
            usage_parser_cell: std::sync::OnceLock::new(),
        }
    }

    /// Evaluate the plugin's (settings, notifications) schemas once via QuickJS.
    fn compute_schemas(&self) -> (Vec<SettingField>, Vec<SettingField>) {
        let settings = self
            .try_call::<Vec<SettingField>>("settingsSchema", &[])
            .unwrap_or_else(|| manifest_settings_sections(&self.manifest));
        let notifications = self
            .try_call::<Vec<SettingField>>("notificationsSchema", &[])
            .unwrap_or_default();
        (settings, notifications)
    }

    /// Give the provider a registry handle so it can look up its native
    /// fallback for unexported capability methods. Set by the loader/watcher.
    pub fn set_registrar(
        &mut self,
        registry: Arc<dyn crate::host_traits::PluginProviderRegistrar>,
    ) {
        self.registry = Some(registry);
    }

    /// Whether the JS bundle exports `method` — distinguishes "not implemented"
    /// (delegate to native) from "implemented, returned empty".
    fn has_js(&self, method: &str) -> bool {
        super::vm::has_method(&self.vm, method)
    }

    /// The native provider of this id displaced by this JS plugin, if any.
    fn native(&self) -> Option<Arc<dyn AgentProvider>> {
        self.registry.as_ref()?.native_fallback(&self.manifest.id)
    }

    /// Manifest-declared hook roster resolved against the closed route set.
    /// `None` = nothing declared; `Err` = declared but invalid (fails closed).
    fn declared_hook_event_roster(
        &self,
    ) -> Option<Result<Vec<provider::HookEventBinding>, String>> {
        self.manifest
            .spawn
            .hook_event_roster
            .as_deref()
            .map(super::resolve_hook_event_roster)
    }

    pub fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    pub fn vm(&self) -> &Arc<PluginVm> {
        &self.vm
    }

    /// Try to call a plugin method, returning `None` if it isn't exported
    /// and logging anything else. Swallowing missing-function lets plugin
    /// authors omit methods they don't implement.
    fn try_call<R: for<'de> Deserialize<'de>>(&self, method: &str, args: &[Value]) -> Option<R> {
        match call_json::<R>(&self.vm, method, args) {
            Ok(v) => Some(v),
            Err(VmError::MissingFunction(_)) => None,
            Err(VmError::Script(msg)) if msg.contains("not a function") => None,
            Err(e) => {
                log::warn!("[plugin {}] {} failed: {e}", self.manifest.id, method);
                None
            }
        }
    }

    fn extend_parsed_session_chunk(&self, out: &mut Vec<SessionMessage>, chunk: &str) {
        if let Some(parsed) =
            self.try_call::<Vec<SerdeSessionMessage>>("parseSessionMessages", &[json!(chunk)])
        {
            out.extend(parsed.into_iter().map(SessionMessage::from));
        }
    }

    /// Call a plugin method expected to return `{ok: bool, error?: string}`.
    /// Returns `None` when the plugin doesn't export the method (so the
    /// caller can fall back to a manifest-driven default); returns
    /// `Some(Ok)` / `Some(Err)` once the plugin replies.
    fn try_call_result(&self, method: &str, args: &[Value]) -> Option<Result<(), String>> {
        self.try_call::<serde_json::Value>(method, args)
            .map(|v| result_from_js(&v))
    }
}

/// `usageSources` wire row: a path plus the session that scopes a shared log.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireUsageSource {
    path: String,
    #[serde(default)]
    session_id: Option<String>,
}

#[async_trait::async_trait]
impl AgentProvider for JsProvider {
    fn id(&self) -> &str {
        &self.manifest.id
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

    fn verify_submit_strategy(&self) -> VerifySubmitStrategy {
        (&self.manifest.spawn.verify_submit).into()
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
            }
        }
        if out.is_empty() && crate::detector::detect_interactive_prompt(text) {
            out.push(crate::detector::AgentEvent::PermissionRequest);
        }
        out
    }

    fn parse_prompt(&self, text: &str) -> Option<PendingPrompt> {
        // Plugin returns `{question, options: [[id, label]]}` or null.
        if let Some(v) = self.try_call::<Option<Value>>("parsePrompt", &[json!(text)]) {
            return v.and_then(prompt_parse::parsed_prompt_from_js);
        }
        // Fallback to universal detector if plugin doesn't implement.
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
        // Default to the trait default (arrow-down + Enter).
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
        let steps = self
            .try_call::<Option<Vec<String>>>("directoryTrustPromptResponse", &[json!(screen)])??;
        use base64::Engine;
        let engine = base64::engine::general_purpose::STANDARD;
        let decoded = steps
            .iter()
            .map(|step| engine.decode(step.as_bytes()).ok())
            .collect::<Option<Vec<_>>>()
            .unwrap_or_default();
        Some(crate::provider::DirectoryTrustPromptResponse::from_wire_steps(decoded))
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

    fn session_file_path(&self, cwd: &str, session_id: &str) -> Option<PathBuf> {
        // Hook-reported evidence wins over any reconstruction; absence falls through.
        if let Some(stored) = crate::session_evidence::stored_transcript_path(session_id) {
            return Some(stored);
        }
        self.owned_session_file_path(cwd, session_id)
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

    fn session_exists(&self, cwd: &str, session_id: &str) -> bool {
        if let Some(b) = self.try_call::<bool>("sessionExists", &[json!(cwd), json!(session_id)]) {
            return b;
        }
        self.owned_session_file_path(cwd, session_id)
            .map(|p| p.exists())
            .unwrap_or(false)
    }

    fn detect_session_model(&self, cwd: &str, session_id: &str) -> Option<String> {
        self.try_call::<Option<String>>("detectSessionModel", &[json!(cwd), json!(session_id)])
            .flatten()
            .map(|model| {
                let resolved = crate::model_catalog::resolve_model(self.id(), Some(&model));
                if resolved.fell_back {
                    model
                } else {
                    resolved.id
                }
            })
    }

    fn detect_session_reasoning_effort(&self, cwd: &str, session_id: &str) -> Option<String> {
        self.try_call::<Option<String>>(
            "detectSessionReasoningEffort",
            &[json!(cwd), json!(session_id)],
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
        let cfg = self.manifest.sessions.as_ref()?;
        let dir = self.sessions_dir(cwd)?;
        if !dir.exists() {
            return None;
        }
        let recency_limit = max_age.unwrap_or_else(|| Duration::from_secs(300));
        let now = std::time::SystemTime::now();
        let dot_ext = format!(".{}", cfg.file_extension);
        let mut candidates: Vec<(String, std::time::SystemTime)> = Vec::new();

        let entries = std::fs::read_dir(&dir).ok()?;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let Some(stem) = name.strip_suffix(&dot_ext) else {
                continue;
            };
            // Heuristic: session ids look uuid-ish (>=32 chars with a dash).
            if stem.len() < 32 || !stem.contains('-') {
                continue;
            }
            if exclude.iter().any(|e| e == stem) {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if let Ok(modified) = meta.modified() {
                if let Ok(age) = now.duration_since(modified) {
                    if age > recency_limit {
                        continue;
                    }
                }
            }
            let sort_key = meta.created().or_else(|_| meta.modified()).ok()?;
            candidates.push((stem.to_string(), sort_key));
        }
        match candidates.as_slice() {
            [(id, _)] => Some(id.clone()),
            _ => None,
        }
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

    fn session_jsonl_path(&self, cwd: &str, session_id: &str) -> Option<std::path::PathBuf> {
        // Hook-reported evidence wins over any reconstruction; absence falls through.
        if let Some(stored) = crate::session_evidence::stored_transcript_path(session_id) {
            return Some(stored);
        }
        if !self.has_js("sessionJsonlPath") {
            return self
                .native()
                .and_then(|n| n.session_jsonl_path(cwd, session_id));
        }
        let resolved = match self.manifest.sessions.as_ref().map(|s| s.cwd_encoding) {
            Some(super::CwdEncoding::DashEncoded) => codeterm_paths::canonical_cwd(cwd),
            _ => cwd.to_string(),
        };
        self.try_call::<Option<String>>(
            "sessionJsonlPath",
            &[serde_json::json!(resolved), serde_json::json!(session_id)],
        )
        .flatten()
        .filter(|path| !path.is_empty())
        .map(std::path::PathBuf::from)
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
        if self.has_js("readStructuredChat") {
            return self
                .try_call(
                    "readStructuredChat",
                    &[json!(cwd), json!(session_id), json!(cursor)],
                )
                .unwrap_or_default();
        }
        match self.native() {
            Some(n) => n.read_structured_chat(cwd, session_id, cursor),
            None => codeterm_types::agent::chat_types::StructuredChat::default(),
        }
    }

    fn supports_chat_ui(&self) -> bool {
        // structured-capable, or a native fallback that serves chat.
        self.has_js("sessionJsonlPath")
            || self.uses_structured_chat()
            || self.native().map(|n| n.supports_chat_ui()).unwrap_or(false)
    }

    fn parse_session_delta(
        &self,
        path: &std::path::Path,
        from_offset: u64,
    ) -> codeterm_types::agent::chat_types::SessionDelta {
        use codeterm_types::agent::chat_types::SessionDelta;
        // Seek-based and capped: session files reach hundreds of MB, and this runs on every tailer poll.
        let read = crate::jsonl_window::read_records_from(
            path,
            from_offset,
            crate::jsonl_window::DELTA_CHUNK_BYTES,
        );
        let window = match read {
            Ok(w) => w,
            Err(_) => {
                return SessionDelta {
                    new_offset: from_offset,
                    messages: vec![],
                }
            }
        };
        // The reader holds back a partial trailing line, so the plugin never sees it and byte/code-unit mismatches in JS string indexing can't corrupt the offset.
        if window.bytes.is_empty() {
            return SessionDelta {
                new_offset: window.new_offset,
                messages: vec![],
            };
        }
        let consumed_bytes = window.new_offset - from_offset;
        // land inside line bodies, which the plugin's per-line JSON.parse will reject. Byte-count is no longer trusted from JS; we computed it above.
        let chunk = String::from_utf8_lossy(&window.bytes).to_string();
        let context = serde_json::json!({
            "session_key": path.to_string_lossy(),
            "from_offset": from_offset,
        });
        #[derive(serde::Deserialize)]
        struct JsDelta {
            messages: Vec<serde_json::Value>,
        }
        let parsed = self
            .try_call::<JsDelta>("parseSessionDelta", &[serde_json::json!(chunk), context])
            .and_then(|d| crate::jsonl_window::rows_from_plugin(d.messages));
        match parsed {
            Some((mut messages, record_indexes)) => {
                crate::jsonl_window::anchor_rows(
                    &mut messages,
                    &record_indexes,
                    &window.bytes,
                    from_offset,
                );
                SessionDelta {
                    new_offset: from_offset + consumed_bytes,
                    messages,
                }
            }
            None => {
                // messages, but the tailer escapes the infinite-retry trap. The warning from try_call is enough audit trail.
                log::warn!(
                    "[parse_session_delta] plugin '{}' returned no delta for {} bytes at offset {} — skipping window",
                    self.manifest.id, consumed_bytes, from_offset
                );
                SessionDelta {
                    new_offset: from_offset + consumed_bytes,
                    messages: vec![],
                }
            }
        }
    }

    fn parse_interactive_events(
        &self,
        msg: &codeterm_types::agent::chat_types::RichMessage,
    ) -> Vec<codeterm_types::agent::chat_types::InteractiveElement> {
        if self.manifest.spawn.interactive_events && self.has_js("parseInteractiveEvents") {
            let arg = match serde_json::to_value(msg) {
                Ok(v) => v,
                Err(_) => return Vec::new(),
            };
            return self
                .try_call::<Vec<codeterm_types::agent::chat_types::InteractiveElement>>(
                    "parseInteractiveEvents",
                    &[arg],
                )
                .unwrap_or_default();
        }
        self.native()
            .map(|n| n.parse_interactive_events(msg))
            .unwrap_or_default()
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
        if model.is_empty() {
            return Vec::new();
        }
        let tmpl = &self.manifest.commands.model_launch_args_format;
        if tmpl.is_empty() {
            return vec!["--model".into(), model.into()];
        }
        tmpl.iter().map(|t| t.replace("{model}", model)).collect()
    }

    fn reasoning_efforts(&self) -> Vec<ReasoningEffortInfo> {
        crate::model_catalog::resolve_reasoning_efforts(&self.manifest.id)
    }

    fn default_reasoning_effort(&self) -> Option<String> {
        crate::model_catalog::resolve_default_reasoning_effort(&self.manifest.id)
            .or_else(|| self.manifest.default_reasoning_effort.clone())
    }

    fn reasoning_effort_launch_args(&self, effort: &str) -> Vec<String> {
        if effort.is_empty() {
            return Vec::new();
        }
        self.manifest
            .commands
            .reasoning_effort_launch_args_format
            .iter()
            .map(|t| t.replace("{effort}", effort))
            .collect()
    }

    fn compact_command(&self) -> Option<&str> {
        self.manifest.commands.compact.as_deref()
    }

    fn provider_self_update_capability(&self) -> Option<provider::ProviderSelfUpdateCapability> {
        if !self.has_js("providerSelfUpdateCapability") {
            return self
                .native()
                .and_then(|native| native.provider_self_update_capability());
        }
        self.try_call("providerSelfUpdateCapability", &[])
    }

    fn fresh_context_command(&self) -> Option<&str> {
        self.manifest.commands.fresh_context.as_deref()
    }

    fn exit_command(&self) -> Option<&str> {
        self.manifest.commands.exit.as_deref()
    }

    fn stop_keystroke(&self) -> Option<&str> {
        self.manifest.commands.stop_key.as_deref()
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

    fn model_switch_format(&self) -> Option<&str> {
        self.manifest.commands.model_switch_format.as_deref()
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

    fn available_models(&self) -> Vec<ModelInfo> {
        crate::model_catalog::resolve_models(&self.manifest.id)
    }

    async fn discover_models(&self) -> Option<Vec<ModelInfo>> {
        // `null` is the plugin's honest "no genuine discovery" signal — it must
        // not deserialize-fail into warn noise, and never falls back to a seed.
        self.try_call::<Option<Vec<SerdeModelInfo>>>("discoverModels", &[])
            .flatten()
            .map(|models| models.iter().map(ModelInfo::from).collect())
    }

    fn default_model(&self) -> Option<String> {
        crate::model_catalog::resolve_default(&self.manifest.id)
            .or_else(|| self.manifest.default_model.clone())
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
        super::host_credentials::declared_files(&self.manifest.credentials)
    }

    fn settings_schema(&self) -> Vec<SettingField> {
        if !self.has_custom_settings_schema {
            return manifest_settings_sections(&self.manifest);
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
        // manifest.rule.path when present — keeps the contract honest for plugins that declared the path but skipped the JS.
        if let Some(r) = self.try_call_result("installBridgeRule", &[json!(payload)]) {
            return r;
        }
        let Some(rule) = self.manifest.rule.as_ref() else {
            return Ok(());
        };
        let path = PluginManifest::expand_home(&rule.path)
            .ok_or_else(|| "rule.path: home dir not resolvable".to_string())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create {}: {e}", parent.display()))?;
        }
        std::fs::write(&path, payload).map_err(|e| format!("write {}: {e}", path.display()))
    }

    fn uninstall_bridge_rule(&self) -> Result<(), String> {
        if let Some(r) = self.try_call_result("uninstallBridgeRule", &[]) {
            return r;
        }
        let Some(rule) = self.manifest.rule.as_ref() else {
            return Ok(());
        };
        let Some(path) = PluginManifest::expand_home(&rule.path) else {
            return Ok(());
        };
        if !path.exists() {
            return Ok(());
        }
        std::fs::remove_file(&path).map_err(|e| format!("remove {}: {e}", path.display()))
    }

    fn check_integration(&self) -> serde_json::Value {
        if let Some(Err(error)) = self.declared_hook_event_roster() {
            return serde_json::json!({
                "status": "error",
                "hooks_installed": false,
                "hook_event_roster_error": error,
            });
        }
        self.try_call::<serde_json::Value>("checkIntegration", &[])
            .unwrap_or_else(|| serde_json::json!({ "status": "ok" }))
    }

    fn integration_kind(&self) -> IntegrationKind {
        self.manifest
            .integration
            .as_ref()
            .map_or(IntegrationKind::None, |integration| integration.kind)
    }

    fn install_integration(
        &self,
        api_port: u16,
        options: &crate::provider::IntegrationInstallOptions,
    ) -> Result<(), String> {
        if !options.consent {
            return Err(
                "provider integration installation requires explicit local-user consent".into(),
            );
        }
        if let Some(Err(error)) = self.declared_hook_event_roster() {
            return Err(error);
        }
        self.try_call_result("installIntegration", &[json!(api_port), json!(options)])
            .unwrap_or(Ok(()))
    }

    fn uninstall_integration(&self) -> Result<(), String> {
        self.try_call_result("uninstallIntegration", &[])
            .unwrap_or(Ok(()))
    }

    fn restore_integration(&self) -> Result<(), String> {
        self.try_call_result("restoreIntegration", &[])
            .unwrap_or_else(|| Err("provider integration has no recoverable backup".into()))
    }

    fn integration_trust_grant_capability(
        &self,
    ) -> Option<crate::provider::IntegrationTrustGrantCapability> {
        self.manifest
            .integration
            .as_ref()?
            .trust_grant
            .as_ref()
            .map(|grant| crate::provider::IntegrationTrustGrantCapability {
                managed_keys: grant.managed_keys.clone(),
            })
    }

    fn grant_integration_trust(&self, managed_keys: &[String]) -> serde_json::Value {
        self.try_call::<serde_json::Value>("grantIntegrationTrust", &[json!(managed_keys)])
            .unwrap_or_else(|| {
                serde_json::json!({
                    "granted": false,
                    "degraded": true,
                    "reason": "plugin exports no grantIntegrationTrust"
                })
            })
    }

    fn ensure_integration_on_startup(&self, api_port: u16) {
        let _ =
            self.try_call::<serde_json::Value>("ensureIntegrationOnStartup", &[json!(api_port)]);
    }
}

#[cfg(test)]
mod tests;
