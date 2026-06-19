//! `JsProvider` — generic `AgentProvider` impl over a `PluginVm`.
//!
//! Every method dispatches in one of three ways:
//!
//! * **Manifest data** — `id`, `display_name`, `models`, slash commands,
//!   icons, readiness/verify/task-delivery enums, etc. Read straight off
//!   the parsed `plugin.json`. Zero allocations on the hot path.
//! * **JS function call** — `detect_from_title`, `detect_from_output`,
//!   `build_launch_command`, `parse_session_messages`, etc. Pure logic
//!   that varies per provider lives in `plugin.js`; we invoke through
//!   `call_json` and surface a sensible default if the function isn't
//!   exported.
//! * **Rust I/O driven by manifest config** — `session_file_path`,
//!   `enumerate_sessions`, `detect_session_id`, `install_bridge_rule`,
//!   `install_integration`. Filesystem and process work stays in Rust
//!   because QuickJS has no `fs`/`process`; the manifest tells us where
//!   to look, and we hand the raw file text to JS for parsing.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};

/// CamelCase mirror of `SessionMetadata` so plugin authors can return
/// `{ sessionId, projectPath, ... }` instead of snake_case.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SerdeSessionMetadata {
    session_id: String,
    provider: Option<String>,
    #[serde(default)]
    project_path: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    file_path: String,
    #[serde(default)]
    created_ts: Option<i64>,
    #[serde(default)]
    modified_ts: Option<i64>,
}

impl SerdeSessionMetadata {
    fn into_session_metadata(self, fallback_provider: &str) -> SessionMetadata {
        SessionMetadata {
            session_id: self.session_id,
            provider: self
                .provider
                .unwrap_or_else(|| fallback_provider.to_string()),
            project_path: self.project_path,
            title: self.title,
            summary: self.summary,
            file_path: self.file_path,
            created_ts: self.created_ts,
            modified_ts: self.modified_ts,
        }
    }
}

/// CamelCase mirror of `SessionMessage` so plugin authors don't have to
/// remember `is_meta` over `isMeta`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SerdeSessionMessage {
    role: String,
    text: String,
    #[serde(default)]
    ts: Option<i64>,
    #[serde(default)]
    is_meta: bool,
}

impl From<SerdeSessionMessage> for SessionMessage {
    fn from(s: SerdeSessionMessage) -> Self {
        SessionMessage {
            role: s.role,
            text: s.text,
            ts: s.ts,
            is_meta: s.is_meta,
        }
    }
}

use crate::agent::provider::{
    self, AgentLaunchParams, AgentProvider, ModelInfo, ProviderTrust, ReadinessStrategy,
    SessionIdInjection, SessionMessage, SessionMetadata, SettingField, TaskDelivery,
    VerifySubmitStrategy,
};

use super::{
    call_json, manifest_settings_sections, PluginManifest, PluginVm, SerdeModelInfo, VmError,
};

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
    registry: Option<Arc<crate::agent::registry::AgentRegistry>>,
    /// settingsSchema/notificationsSchema are pure constants in the plugin but
    /// `describe()` (called on every /api/agents/providers poll) re-evaluated
    /// them through QuickJS every time — a profiled CPU hot path. Compute once.
    schema_cache: std::sync::OnceLock<(Vec<SettingField>, Vec<SettingField>)>,
    /// VM-backed usage parser, built once when the plugin exports `parseUsage`.
    /// `usage_parser()` hands out a reference, so the adapter must live on the
    /// provider.
    usage_parser_cell: std::sync::OnceLock<Option<VmUsageParser>>,
}

/// Bridges the `UsageParser` trait to a plugin's `parseUsage(rawText, nowMs)`
/// export. The plugin returns a `UsageSnapshot`-shaped object (snake_case
/// fields) or null when nothing was recognized.
struct VmUsageParser {
    vm: Arc<PluginVm>,
    plugin_id: String,
}

impl crate::agent::usage_parser::UsageParser for VmUsageParser {
    fn parse(
        &self,
        raw_text: &str,
    ) -> Result<
        crate::agent::usage_parser::UsageSnapshot,
        crate::agent::usage_parser::UsageParseError,
    > {
        use crate::agent::usage_parser::UsageParseError;
        let now_ms = chrono::Utc::now().timestamp_millis();
        match call_json::<Option<crate::agent::usage_parser::UsageSnapshot>>(
            &self.vm,
            "parseUsage",
            &[json!(raw_text), json!(now_ms)],
        ) {
            Ok(Some(snap)) => Ok(snap),
            Ok(None) => Err(UsageParseError::NothingFound),
            Err(e) => Err(UsageParseError::Other(format!(
                "[plugin {}] parseUsage failed: {e}",
                self.plugin_id
            ))),
        }
    }
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
            crate::agent::model_catalog::register(
                &manifest.id,
                crate::agent::model_catalog::RegisteredDefaults {
                    models: manifest.models.iter().map(ModelInfo::from).collect(),
                    default: manifest.default_model.clone(),
                },
            );
        }
        Self {
            vm,
            manifest,
            trust,
            registry: None,
            schema_cache: std::sync::OnceLock::new(),
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
    pub fn set_registry(&mut self, registry: Arc<crate::agent::registry::AgentRegistry>) {
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

    /// Call a plugin method expected to return `{ok: bool, error?: string}`.
    /// Returns `None` when the plugin doesn't export the method (so the
    /// caller can fall back to a manifest-driven default); returns
    /// `Some(Ok)` / `Some(Err)` once the plugin replies.
    fn try_call_result(&self, method: &str, args: &[Value]) -> Option<Result<(), String>> {
        self.try_call::<serde_json::Value>(method, args)
            .map(|v| result_from_js(&v))
    }

    fn sessions_dir(&self, cwd: &str) -> Option<PathBuf> {
        let cfg = self.manifest.sessions.as_ref()?;
        let root = PluginManifest::expand_home(&cfg.root_dir)?;
        let leaf = match cfg.cwd_encoding {
            // Agents (e.g. claude) canonicalize the cwd before encoding the
            // project dir — on macOS `/var/...` resolves to `/private/var/...`,
            // so a raw cwd would encode to a dir that doesn't exist. Match it
            // (verbatim-prefix stripped on Windows, see `canonical_cwd`).
            super::CwdEncoding::DashEncoded => encode_cwd_dashed(&crate::paths::canonical_cwd(cwd)),
            super::CwdEncoding::None => cwd.to_string(),
        };
        Some(root.join(leaf))
    }
}

fn encode_cwd_dashed(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect()
}

fn launch_params_to_json(p: &AgentLaunchParams) -> Value {
    json!({
        "agentName": p.agent_name,
        "task": p.task,
        "args": p.args,
        "skipPermissions": p.skip_permissions,
        "sessionId": p.session_id,
    })
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
        self.try_call::<String>("buildLaunchCommand", &[launch_params_to_json(params)])
            .unwrap_or_default()
    }

    fn readiness_strategy(&self) -> ReadinessStrategy {
        (&self.manifest.spawn.readiness_strategy).into()
    }

    fn task_delivery(&self) -> TaskDelivery {
        self.manifest.spawn.task_delivery.into()
    }

    fn task_input_warmup(&self) -> Duration {
        Duration::from_millis(self.manifest.spawn.task_input_warmup_ms)
    }

    fn verify_submit_strategy(&self) -> VerifySubmitStrategy {
        (&self.manifest.spawn.verify_submit).into()
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

    fn classify_pane_state(
        &self,
        snapshot: &crate::agent::pane_state::PaneStateSnapshot,
    ) -> Option<crate::agent::pane_state::PaneStateClassification> {
        if !self.has_js("classifyPaneState") {
            return self.native().and_then(|n| n.classify_pane_state(snapshot));
        }
        let arg = serde_json::to_value(snapshot).ok()?;
        self.try_call::<Option<crate::agent::pane_state::PaneStateClassification>>(
            "classifyPaneState",
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

    fn detect_events(&self, text: &str) -> Vec<crate::agent::detector::AgentEvent> {
        // Plugin returns a vec of event-name strings ("permission_request", "completed", ...). For v1 we only support permission_request via the
        // universal detector — anything else returned by the plugin is ignored until the AgentEvent enum grows new variants.
        let names: Vec<String> = self
            .try_call::<Vec<String>>("detectEvents", &[json!(text)])
            .unwrap_or_default();
        let mut out = Vec::new();
        for n in names {
            if n == "permission_request" {
                out.push(crate::agent::detector::AgentEvent::PermissionRequest);
            }
        }
        if out.is_empty() && crate::agent::detector::detect_interactive_prompt(text) {
            out.push(crate::agent::detector::AgentEvent::PermissionRequest);
        }
        out
    }

    fn parse_prompt(&self, text: &str) -> Option<(String, Vec<(String, String)>)> {
        // Plugin returns `{question, options: [[id, label]]}` or null.
        if let Some(v) = self.try_call::<Option<Value>>("parsePrompt", &[json!(text)]) {
            if let Some(obj) = v {
                let question = obj.get("question").and_then(|q| q.as_str())?.to_string();
                let options = obj.get("options").and_then(|o| o.as_array())?;
                let mut parsed = Vec::new();
                for opt in options {
                    let pair = opt.as_array()?;
                    if pair.len() != 2 {
                        return None;
                    }
                    let id = pair[0].as_str()?.to_string();
                    let label = pair[1].as_str()?.to_string();
                    parsed.push((id, label));
                }
                return Some((question, parsed));
            }
            return None;
        }
        // Fallback to universal detector if plugin doesn't implement.
        crate::agent::detector::parse_permission_prompt(text)
    }

    fn build_prompt_response(&self, option_index: usize) -> Vec<Vec<u8>> {
        // Plugin returns a list of base64-encoded byte strings, one per PTY write step. Lets plugins emit vim j/k, arrow keys, escape sequences — whatever the TUI expects.
        if let Some(steps) =
            self.try_call::<Vec<String>>("buildPromptResponse", &[json!(option_index)])
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

    fn session_file_path(&self, cwd: &str, session_id: &str) -> Option<PathBuf> {
        if let Some(p) =
            self.try_call::<Option<String>>("sessionFilePath", &[json!(cwd), json!(session_id)])
        {
            return p.map(PathBuf::from);
        }
        let cfg = self.manifest.sessions.as_ref()?;
        let dir = self.sessions_dir(cwd)?;
        Some(dir.join(format!("{}.{}", session_id, cfg.file_extension)))
    }

    fn session_exists(&self, cwd: &str, session_id: &str) -> bool {
        if let Some(b) = self.try_call::<bool>("sessionExists", &[json!(cwd), json!(session_id)]) {
            return b;
        }
        self.session_file_path(cwd, session_id)
            .map(|p| p.exists())
            .unwrap_or(false)
    }

    fn detect_session_model(&self, cwd: &str, session_id: &str) -> Option<String> {
        self.try_call::<Option<String>>("detectSessionModel", &[json!(cwd), json!(session_id)])
            .flatten()
            .map(|model| crate::agent::model_catalog::resolve_model(self.id(), Some(&model)).id)
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

    fn detect_session_id(
        &self,
        cwd: &str,
        exclude: &[String],
        max_age: Option<Duration>,
    ) -> Option<String> {
        // Plugin can do its own detection (sqlite, filename heuristics, whatever its provider needs). Falls back to the generic dir walk driven by
        // manifest.sessions.* when the plugin doesn't implement `detectSessionId`.
        // None means "default recency window" (300s, same as native providers) —
        // never 0/unlimited: plugins treat 0 as "no cutoff", which let any stale
        // rollout in the cwd win the latch.
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
        // Identity-bound, never "newest-for-cwd" (capability rule #3): mirror
        // Claude's native guard. When 2+ recent sessions are viable candidates
        // (none pinned by the exclude list), refuse — handing the pane the
        // newest would bind it to a SIBLING's session. The exclude list
        // collapses the set as siblings get bound, so the last unclaimed
        // session still resolves cleanly.
        match candidates.as_slice() {
            [(id, _)] => Some(id.clone()),
            _ => None,
        }
    }

    fn enumerate_sessions(&self) -> Vec<SessionMetadata> {
        // Plugin can enumerate via sqlite, recursive walk, network call, whatever. Falls back to the manifest-driven single-level listing when the
        // plugin doesn't implement `enumerateSessions`.
        if let Some(list) = self.try_call::<Vec<SerdeSessionMetadata>>("enumerateSessions", &[]) {
            return list
                .into_iter()
                .map(|m| m.into_session_metadata(&self.manifest.id))
                .collect();
        }
        let Some(cfg) = self.manifest.sessions.as_ref() else {
            return Vec::new();
        };
        let Some(root) = PluginManifest::expand_home(&cfg.root_dir) else {
            return Vec::new();
        };
        let Ok(entries) = std::fs::read_dir(&root) else {
            return Vec::new();
        };
        let dot_ext = format!(".{}", cfg.file_extension);
        let mut out = Vec::new();
        for entry in entries.flatten() {
            let Ok(ft) = entry.file_type() else { continue };
            if !ft.is_dir() {
                continue;
            }
            let dir_path = entry.path();
            let Ok(files) = std::fs::read_dir(&dir_path) else {
                continue;
            };
            for f in files.flatten() {
                let name = f.file_name().to_string_lossy().to_string();
                let Some(stem) = name.strip_suffix(&dot_ext) else {
                    continue;
                };
                if stem.len() < 32 || !stem.contains('-') {
                    continue;
                }
                let file_path_buf = f.path();
                let meta = f.metadata().ok();
                let created_ts = meta
                    .as_ref()
                    .and_then(|m| provider::fs_time_to_unix(m.created().ok()));
                let modified_ts = meta
                    .as_ref()
                    .and_then(|m| provider::fs_time_to_unix(m.modified().ok()));
                // Ask the plugin to dig title/summary/project_path out of the session file. We pass the path string — the plugin may choose to ignore (e.g.
                // plain JSONL provider that only cares about per-line cwd via parse_session_messages).
                let extra: Option<Value> =
                    self.try_call("describeSession", &[json!(file_path_buf.to_string_lossy())]);
                let title = extra
                    .as_ref()
                    .and_then(|v| v.get("title"))
                    .and_then(|s| s.as_str())
                    .map(String::from);
                let summary = extra
                    .as_ref()
                    .and_then(|v| v.get("summary"))
                    .and_then(|s| s.as_str())
                    .map(String::from);
                let project_path = extra
                    .as_ref()
                    .and_then(|v| v.get("projectPath"))
                    .and_then(|s| s.as_str())
                    .map(String::from);

                out.push(SessionMetadata {
                    session_id: stem.to_string(),
                    provider: self.manifest.id.clone(),
                    project_path,
                    title,
                    summary,
                    file_path: file_path_buf.to_string_lossy().to_string(),
                    created_ts,
                    modified_ts,
                });
            }
        }
        out
    }

    fn parse_session_messages(&self, file_path: &Path) -> Vec<SessionMessage> {
        if !self.has_js("parseSessionMessages") {
            return match self.native() {
                Some(n) => n.parse_session_messages(file_path),
                None => Vec::new(),
            };
        }
        let Ok(text) = std::fs::read_to_string(file_path) else {
            return Vec::new();
        };
        self.try_call::<Vec<SerdeSessionMessage>>("parseSessionMessages", &[json!(text)])
            .map(|v| v.into_iter().map(SessionMessage::from).collect())
            .unwrap_or_default()
    }

    fn session_jsonl_path(&self, cwd: &str, session_id: &str) -> Option<std::path::PathBuf> {
        if !self.has_js("sessionJsonlPath") {
            return self
                .native()
                .and_then(|n| n.session_jsonl_path(cwd, session_id));
        }
        // The sandboxed JS encodes the cwd but can't touch the FS, so canonicalize
        // here first — same as `sessions_dir`. Without this the tailer watches a
        // symlink alias dir (e.g. `/tmp`) while the agent writes to the resolved
        // one (`/private/tmp`), so live deltas never fire. (#163)
        let resolved = match self.manifest.sessions.as_ref().map(|s| s.cwd_encoding) {
            Some(super::CwdEncoding::DashEncoded) => crate::paths::canonical_cwd(cwd),
            _ => cwd.to_string(),
        };
        let s: Option<String> = self.try_call(
            "sessionJsonlPath",
            &[serde_json::json!(resolved), serde_json::json!(session_id)],
        );
        s.filter(|p| !p.is_empty()).map(std::path::PathBuf::from)
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
    ) -> crate::agent::chat_types::StructuredChat {
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
            None => crate::agent::chat_types::StructuredChat::default(),
        }
    }

    fn supports_chat_ui(&self) -> bool {
        // Capability-based, no provider-id list: JSONL-capable (exports a path),
        // structured-capable, or a native fallback that serves chat.
        self.has_js("sessionJsonlPath")
            || self.uses_structured_chat()
            || self.native().map(|n| n.supports_chat_ui()).unwrap_or(false)
    }

    fn parse_session_delta(
        &self,
        path: &std::path::Path,
        from_offset: u64,
    ) -> crate::agent::chat_types::SessionDelta {
        use crate::agent::chat_types::SessionDelta;
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => {
                return SessionDelta {
                    new_offset: from_offset,
                    messages: vec![],
                }
            }
        };
        let total = bytes.len() as u64;
        if from_offset >= total {
            return SessionDelta {
                new_offset: from_offset,
                messages: vec![],
            };
        }
        let slice = &bytes[from_offset as usize..];
        // Find the LAST '\n' byte in the slice — everything up to and including it is a guaranteed complete-line region; bytes after are a partial
        // line we hold back for the next poll. The plugin never sees the partial part, so byte/code-unit mismatches in JS string indexing can't corrupt the offset.
        let last_nl = match slice.iter().rposition(|&b| b == b'\n') {
            Some(idx) => idx,
            None => {
                return SessionDelta {
                    new_offset: from_offset,
                    messages: vec![],
                }
            }
        };
        let consumed_bytes = (last_nl + 1) as u64;
        let complete_slice = &slice[..consumed_bytes as usize];
        // UTF-8 decode for JS. lossy is OK here — by definition we only do it on a slice that ends on a \n byte boundary; any U+FFFD substitutions
        // land inside line bodies, which the plugin's per-line JSON.parse will reject. Byte-count is no longer trusted from JS; we computed it above.
        let chunk = String::from_utf8_lossy(complete_slice).to_string();
        #[derive(serde::Deserialize)]
        struct JsDelta {
            messages: Vec<crate::agent::chat_types::RichMessage>,
        }
        let parsed: Option<JsDelta> =
            self.try_call("parseSessionDelta", &[serde_json::json!(chunk)]);
        match parsed {
            Some(d) => SessionDelta {
                new_offset: from_offset + consumed_bytes,
                messages: d.messages,
            },
            None => {
                // I4: surface plugin failures instead of busy-looping on the same broken bytes. Advance the offset past this chunk anyway — yes, we lose the
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
        msg: &crate::agent::chat_types::RichMessage,
    ) -> Vec<crate::agent::chat_types::InteractiveElement> {
        if self.manifest.spawn.interactive_events && self.has_js("parseInteractiveEvents") {
            let arg = match serde_json::to_value(msg) {
                Ok(v) => v,
                Err(_) => return Vec::new(),
            };
            return self
                .try_call::<Vec<crate::agent::chat_types::InteractiveElement>>(
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
        self.try_call::<Option<String>>(
            "buildResumeCommand",
            &[json!(session_id), json!(skip_permissions)],
        )
        .flatten()
    }

    fn has_hooks(&self) -> bool {
        self.manifest.spawn.has_hooks
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

    fn compact_command(&self) -> Option<&str> {
        self.manifest.commands.compact.as_deref()
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

    fn usage_parser(&self) -> Option<&dyn crate::agent::usage_parser::UsageParser> {
        self.usage_parser_cell
            .get_or_init(|| {
                self.has_js("parseUsage").then(|| VmUsageParser {
                    vm: self.vm.clone(),
                    plugin_id: self.manifest.id.clone(),
                })
            })
            .as_ref()
            .map(|p| p as &dyn crate::agent::usage_parser::UsageParser)
    }

    fn model_switch_format(&self) -> Option<&str> {
        self.manifest.commands.model_switch_format.as_deref()
    }

    fn supports_lazy_skills(&self) -> bool {
        self.manifest.spawn.supports_lazy_skills
    }

    fn icon_hint(&self) -> Option<&str> {
        self.manifest.icon_hint.as_deref()
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

    fn available_models(&self) -> Vec<ModelInfo> {
        crate::agent::model_catalog::resolve_models(&self.manifest.id)
    }

    async fn discover_models(&self) -> Option<Vec<ModelInfo>> {
        self.try_call::<Vec<SerdeModelInfo>>("discoverModels", &[])
            .map(|models| models.iter().map(ModelInfo::from).collect())
    }

    fn default_model(&self) -> Option<String> {
        crate::agent::model_catalog::resolve_default(&self.manifest.id)
            .or_else(|| self.manifest.default_model.clone())
    }

    fn preserves_context_on_resume(&self) -> bool {
        self.manifest.spawn.preserves_context_on_resume
    }

    fn settings_schema(&self) -> Vec<SettingField> {
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
        // Plugin returns { ok: true } or { ok: false, error: "..." }. If the plugin doesn't export `installBridgeRule`, fall back to writing
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
        self.try_call::<serde_json::Value>("checkIntegration", &[])
            .unwrap_or_else(|| serde_json::json!({ "status": "ok" }))
    }

    fn install_integration(&self, api_port: u16) -> Result<(), String> {
        self.try_call_result("installIntegration", &[json!(api_port)])
            .unwrap_or(Ok(()))
    }

    fn uninstall_integration(&self) -> Result<(), String> {
        self.try_call_result("uninstallIntegration", &[])
            .unwrap_or(Ok(()))
    }

    fn ensure_integration_on_startup(&self, api_port: u16) {
        // Plugin owns the dance if it implements `ensureIntegrationOnStartup`; otherwise check-then-install. Either way, swallow errors — a missing or
        // failed hook install must not block startup.
        if let Some(r) = self.try_call_result("ensureIntegrationOnStartup", &[json!(api_port)]) {
            let _ = r;
            return;
        }
        let installed = self
            .check_integration()
            .get("hooks_installed")
            .and_then(|b| b.as_bool())
            .unwrap_or(false);
        if installed {
            return;
        }
        if let Err(e) = self.install_integration(api_port) {
            log::warn!(
                "[js-plugin {}] auto-install integration failed: {e}",
                self.manifest.id
            );
        }
    }
}

fn result_from_js(v: &serde_json::Value) -> Result<(), String> {
    let ok = v.get("ok").and_then(|b| b.as_bool()).unwrap_or(false);
    if ok {
        return Ok(());
    }
    let err = v
        .get("error")
        .and_then(|e| e.as_str())
        .unwrap_or("plugin returned ok=false");
    Err(err.to_string())
}

#[cfg(test)]
mod tests;
