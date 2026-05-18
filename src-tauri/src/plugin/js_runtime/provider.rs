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
            provider: self.provider.unwrap_or_else(|| fallback_provider.to_string()),
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
    self, AgentLaunchParams, AgentProvider, ModelInfo, ReadinessStrategy, SessionIdInjection,
    SessionMessage, SessionMetadata, SettingField, TaskDelivery, VerifySubmitStrategy,
};

use super::{call_json, manifest_settings_sections, PluginManifest, PluginVm, VmError};

/// One generic provider per plugin bundle.
pub struct JsProvider {
    vm: Arc<PluginVm>,
    manifest: PluginManifest,
}

impl JsProvider {
    pub fn new(manifest: PluginManifest, vm: Arc<PluginVm>) -> Self {
        // Register the plugin's models with the catalog so settings UI
        // and `default_model()` resolve through the same plumbing as
        // native providers.
        if !manifest.models.is_empty() {
            crate::agent::model_catalog::register(
                &manifest.id,
                crate::agent::model_catalog::RegisteredDefaults {
                    models: manifest.models.iter().map(ModelInfo::from).collect(),
                    default: manifest.default_model.clone(),
                },
            );
        }
        Self { vm, manifest }
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
                log::warn!(
                    "[plugin {}] {} failed: {e}",
                    self.manifest.id,
                    method
                );
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
            super::CwdEncoding::DashEncoded => encode_cwd_dashed(cwd),
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
        "headless": p.headless,
        "sessionId": p.session_id,
    })
}

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

    fn is_output_noise(&self, text: &str) -> bool {
        self.try_call::<bool>("isOutputNoise", &[json!(text)])
            .unwrap_or(false)
    }

    fn status_bar_rows(&self) -> u16 {
        self.manifest.spawn.status_bar_rows
    }

    fn has_event_markers(&self, chunk: &str) -> bool {
        // Default: arrows / proceed / 1./2. heuristic from trait, but
        // let the plugin override when it has its own fingerprints.
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
        // Plugin returns a vec of event-name strings ("permission_request",
        // "completed", ...). For v1 we only support permission_request via
        // the universal detector — anything else returned by the plugin is
        // simply ignored until the AgentEvent enum grows new variants.
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
        // Plugin returns a list of base64-encoded byte strings, one per
        // PTY write step. Lets plugins emit vim j/k, arrow keys, escape
        // sequences — whatever the TUI expects.
        if let Some(steps) = self.try_call::<Vec<String>>(
            "buildPromptResponse",
            &[json!(option_index)],
        ) {
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
        if let Some(p) = self.try_call::<Option<String>>(
            "sessionFilePath",
            &[json!(cwd), json!(session_id)],
        ) {
            return p.map(PathBuf::from);
        }
        let cfg = self.manifest.sessions.as_ref()?;
        let dir = self.sessions_dir(cwd)?;
        Some(dir.join(format!("{}.{}", session_id, cfg.file_extension)))
    }

    fn session_exists(&self, cwd: &str, session_id: &str) -> bool {
        if let Some(b) = self.try_call::<bool>(
            "sessionExists",
            &[json!(cwd), json!(session_id)],
        ) {
            return b;
        }
        self.session_file_path(cwd, session_id)
            .map(|p| p.exists())
            .unwrap_or(false)
    }

    fn detect_session_model(&self, cwd: &str, session_id: &str) -> Option<String> {
        self.try_call::<Option<String>>(
            "detectSessionModel",
            &[json!(cwd), json!(session_id)],
        )
        .flatten()
    }

    fn detect_active_session(&self, cwd: &str, current_sid: &str) -> Option<String> {
        self.try_call::<Option<String>>(
            "detectActiveSession",
            &[json!(cwd), json!(current_sid)],
        )
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
        // Plugin can do its own detection (sqlite, filename heuristics,
        // whatever its provider needs). Falls back to the generic dir
        // walk driven by manifest.sessions.* when the plugin doesn't
        // implement `detectSessionId`.
        let max_age_ms = max_age.map(|d| d.as_millis() as i64).unwrap_or(0);
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
            let Some(stem) = name.strip_suffix(&dot_ext) else { continue };
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
        candidates.sort_by(|a, b| b.1.cmp(&a.1));
        candidates.into_iter().next().map(|(id, _)| id)
    }

    fn enumerate_sessions(&self) -> Vec<SessionMetadata> {
        // Plugin can enumerate via sqlite, recursive walk, network call,
        // whatever. Falls back to the manifest-driven single-level
        // listing when the plugin doesn't implement `enumerateSessions`.
        if let Some(list) =
            self.try_call::<Vec<SerdeSessionMetadata>>("enumerateSessions", &[])
        {
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
            let Ok(files) = std::fs::read_dir(&dir_path) else { continue };
            for f in files.flatten() {
                let name = f.file_name().to_string_lossy().to_string();
                let Some(stem) = name.strip_suffix(&dot_ext) else { continue };
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
                // Ask the plugin to dig title/summary/project_path out of
                // the session file. We pass the path string — the plugin
                // may choose to ignore (e.g. plain JSONL provider that
                // only cares about per-line cwd via parse_session_messages).
                let extra: Option<Value> = self.try_call(
                    "describeSession",
                    &[json!(file_path_buf.to_string_lossy())],
                );
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
        let Ok(text) = std::fs::read_to_string(file_path) else { return Vec::new() };
        self.try_call::<Vec<SerdeSessionMessage>>("parseSessionMessages", &[json!(text)])
            .map(|v| v.into_iter().map(SessionMessage::from).collect())
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

    fn model_switch_command(&self, model: &str) -> Option<String> {
        self.manifest
            .commands
            .model_switch_format
            .as_ref()
            .map(|fmt| fmt.replace("{model}", model))
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
        self.manifest.models.iter().map(ModelInfo::from).collect()
    }

    fn default_model(&self) -> Option<String> {
        crate::agent::model_catalog::resolve_default(&self.manifest.id)
            .or_else(|| self.manifest.default_model.clone())
    }

    fn preserves_context_on_resume(&self) -> bool {
        self.manifest.spawn.preserves_context_on_resume
    }

    fn settings_schema(&self) -> Vec<SettingField> {
        // Plugin-provided schema (richer) takes precedence; otherwise
        // assemble standard sections from manifest data.
        if let Some(schema) = self.try_call::<Vec<SettingField>>("settingsSchema", &[]) {
            return schema;
        }
        manifest_settings_sections(&self.manifest)
    }

    fn notifications_schema(&self) -> Vec<SettingField> {
        self.try_call::<Vec<SettingField>>("notificationsSchema", &[])
            .unwrap_or_default()
    }

    fn install_bridge_rule(&self, payload: &str) -> Result<(), String> {
        // Plugin returns { ok: true } or { ok: false, error: "..." }.
        // If the plugin doesn't export `installBridgeRule`, fall back to
        // writing manifest.rule.path when present — keeps the contract
        // honest for plugins that declared the path but skipped the JS.
        if let Some(r) = self.try_call_result("installBridgeRule", &[json!(payload)]) {
            return r;
        }
        let Some(rule) = self.manifest.rule.as_ref() else { return Ok(()) };
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
        let Some(rule) = self.manifest.rule.as_ref() else { return Ok(()) };
        let Some(path) = PluginManifest::expand_home(&rule.path) else { return Ok(()) };
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
        // Plugin owns the dance if it implements `ensureIntegrationOnStartup`;
        // otherwise check-then-install. Either way, swallow errors — a
        // missing or failed hook install must not block startup.
        if let Some(r) = self.try_call_result(
            "ensureIntegrationOnStartup",
            &[json!(api_port)],
        ) {
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
mod tests {
    use super::*;
    use serde_json::json;

    fn load(manifest_json: &str, plugin_js: &str) -> JsProvider {
        let mut m: PluginManifest =
            serde_json::from_str(manifest_json).expect("parse manifest");
        m.source_path = Some(PathBuf::from("/tmp/test-plugin/plugin.json"));
        let vm = PluginVm::from_source(&m.id, plugin_js.to_string()).expect("vm");
        JsProvider::new(m, Arc::new(vm))
    }

    const MIN_MANIFEST: &str = r#"{
        "id": "demo",
        "version": "0.1.0",
        "displayName": "Demo",
        "binaryName": "demo",
        "spawn": {
            "readinessStrategy": { "kind": "poll_shell_prompt", "timeoutSecs": 5 },
            "taskDelivery": "pty_input",
            "verifySubmit": { "kind": "none" },
            "statusBarRows": 2,
            "hasHooks": true
        },
        "commands": {
            "compact": "/compact",
            "exit": "/exit",
            "autoApproveFlags": ["--yolo"]
        }
    }"#;

    #[test]
    fn id_display_binary_from_manifest() {
        let p = load(MIN_MANIFEST, "module.exports.default = {};");
        assert_eq!(p.id(), "demo");
        assert_eq!(p.display_name(), "Demo");
        assert_eq!(p.binary_name(), "demo");
    }

    #[test]
    fn binary_name_defaults_to_id_when_omitted() {
        let json = r#"{
            "id": "demo2",
            "version": "0.1.0",
            "displayName": "D2",
            "spawn": {
                "readinessStrategy": { "kind": "fixed_delay", "durationMs": 0 },
                "taskDelivery": "pty_input",
                "verifySubmit": { "kind": "none" }
            }
        }"#;
        let p = load(json, "module.exports.default = {};");
        assert_eq!(p.binary_name(), "demo2");
    }

    #[test]
    fn commands_read_off_manifest() {
        let p = load(MIN_MANIFEST, "module.exports.default = {};");
        assert_eq!(p.compact_command(), Some("/compact"));
        assert_eq!(p.exit_command(), Some("/exit"));
        assert_eq!(p.auto_approve_flags(), vec!["--yolo".to_string()]);
        assert_eq!(p.status_bar_rows(), 2);
        assert!(p.has_hooks());
    }

    #[test]
    fn model_switch_command_substitutes_template() {
        let json = r#"{
            "id": "m",
            "version": "0.1.0",
            "displayName": "M",
            "spawn": {
                "readinessStrategy": { "kind": "fixed_delay", "durationMs": 0 },
                "taskDelivery": "pty_input",
                "verifySubmit": { "kind": "none" }
            },
            "commands": {
                "modelSwitchFormat": "/model {model}"
            }
        }"#;
        let p = load(json, "module.exports.default = {};");
        assert_eq!(p.model_switch_command("opus").as_deref(), Some("/model opus"));
    }

    #[test]
    fn detect_from_title_dispatches_to_js() {
        let js = r#"
            module.exports.default = {
                detectFromTitle(t) { return t.toLowerCase().includes('demo'); }
            };
        "#;
        let p = load(MIN_MANIFEST, js);
        assert!(p.detect_from_title("Running Demo CLI"));
        assert!(!p.detect_from_title("other"));
    }

    #[test]
    fn missing_detect_returns_false() {
        let p = load(MIN_MANIFEST, "module.exports.default = {};");
        assert!(!p.detect_from_title("demo"));
        assert!(!p.detect_from_output("demo"));
    }

    #[test]
    fn build_launch_command_passes_params_to_js() {
        let js = r#"
            module.exports.default = {
                buildLaunchCommand(p) {
                    let s = 'demo';
                    if (p.skipPermissions) s += ' --yolo';
                    if (p.task) s += ' --task ' + JSON.stringify(p.task);
                    if (p.headless) s += ' --headless';
                    return s;
                }
            };
        "#;
        let p = load(MIN_MANIFEST, js);
        let params = AgentLaunchParams {
            task: Some("hello world".into()),
            skip_permissions: true,
            headless: true,
            ..Default::default()
        };
        let cmd = p.build_launch_command(&params);
        assert_eq!(cmd, "demo --yolo --task \"hello world\" --headless");
    }

    #[test]
    fn readiness_and_verify_from_manifest_enums() {
        let p = load(MIN_MANIFEST, "module.exports.default = {};");
        match p.readiness_strategy() {
            ReadinessStrategy::PollShellPrompt { timeout } => {
                assert_eq!(timeout, Duration::from_secs(5));
            }
            _ => panic!(),
        }
        assert!(matches!(p.verify_submit_strategy(), VerifySubmitStrategy::None));
    }

    #[test]
    fn session_file_path_uses_manifest_root() {
        let json = r#"{
            "id": "s",
            "version": "0.1.0",
            "displayName": "S",
            "spawn": {
                "readinessStrategy": { "kind": "fixed_delay", "durationMs": 0 },
                "taskDelivery": "pty_input",
                "verifySubmit": { "kind": "none" }
            },
            "sessions": {
                "rootDir": "/tmp/s-sessions",
                "cwdEncoding": "dash_encoded",
                "fileExtension": "jsonl"
            }
        }"#;
        let p = load(json, "module.exports.default = {};");
        let path = p.session_file_path("/home/x/proj", "abc123").unwrap();
        let s = path.to_string_lossy();
        assert!(s.contains("-home-x-proj"));
        assert!(s.ends_with("abc123.jsonl"), "got {s}");
    }

    #[test]
    fn parse_session_messages_delegates_to_js() {
        let js = r#"
            module.exports.default = {
                parseSessionMessages(text) {
                    return text.split('\n').filter(Boolean).map(l => ({
                        role: 'user', text: l, isMeta: false
                    }));
                }
            };
        "#;
        let p = load(MIN_MANIFEST, js);
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "hello\nworld\n").unwrap();
        let msgs = p.parse_session_messages(tmp.path());
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].text, "hello");
        assert_eq!(msgs[1].text, "world");
    }

    #[test]
    fn extract_session_name_passes_through() {
        let js = r#"
            module.exports.default = {
                extractSessionName(screen) {
                    const m = /session: ([\w-]+)/.exec(screen);
                    return m ? m[1] : null;
                }
            };
        "#;
        let p = load(MIN_MANIFEST, js);
        assert_eq!(
            p.extract_session_name("session: vivid-bhabha-9").as_deref(),
            Some("vivid-bhabha-9")
        );
        assert_eq!(p.extract_session_name("nothing here"), None);
    }

    #[test]
    fn parse_prompt_decodes_options() {
        let js = r#"
            module.exports.default = {
                parsePrompt(_) {
                    return { question: 'Allow?', options: [['y','Yes'],['n','No']] };
                }
            };
        "#;
        let p = load(MIN_MANIFEST, js);
        let (q, opts) = p.parse_prompt("anything").unwrap();
        assert_eq!(q, "Allow?");
        assert_eq!(opts, vec![("y".into(), "Yes".into()), ("n".into(), "No".into())]);
    }

    #[test]
    fn build_prompt_response_decodes_base64_steps() {
        // 'j' = ag==, '\r' = DQ==
        let js = r#"
            module.exports.default = {
                buildPromptResponse(idx) {
                    const steps = [];
                    for (let i = 0; i < idx; i++) steps.push('ag==');
                    steps.push('DQ==');
                    return steps;
                }
            };
        "#;
        let p = load(MIN_MANIFEST, js);
        let steps = p.build_prompt_response(2);
        assert_eq!(steps, vec![b"j".to_vec(), b"j".to_vec(), b"\r".to_vec()]);
    }

    #[test]
    fn settings_schema_from_plugin_overrides_manifest_defaults() {
        let js = r#"
            module.exports.default = {
                settingsSchema() {
                    return [{ kind: 'section', title: 'Custom', fields: [] }];
                }
            };
        "#;
        let p = load(MIN_MANIFEST, js);
        let s = p.settings_schema();
        assert_eq!(s.len(), 1);
        match &s[0] {
            SettingField::Section { title, .. } => assert_eq!(title, "Custom"),
            _ => panic!(),
        }
    }

    #[test]
    fn settings_schema_falls_back_to_manifest_when_plugin_silent() {
        let json = r#"{
            "id": "ms",
            "version": "0.1.0",
            "displayName": "MS",
            "spawn": {
                "readinessStrategy": { "kind": "fixed_delay", "durationMs": 0 },
                "taskDelivery": "pty_input",
                "verifySubmit": { "kind": "none" }
            },
            "models": [
                { "id": "m1", "displayName": "Model 1" }
            ],
            "defaultModel": "m1",
            "commands": {
                "autoApproveFlags": ["--yolo"]
            }
        }"#;
        let p = load(json, "module.exports.default = {};");
        let s = p.settings_schema();
        // session_resume + model_select expected
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn available_models_round_trip_through_manifest() {
        let json = r#"{
            "id": "am",
            "version": "0.1.0",
            "displayName": "AM",
            "spawn": {
                "readinessStrategy": { "kind": "fixed_delay", "durationMs": 0 },
                "taskDelivery": "pty_input",
                "verifySubmit": { "kind": "none" }
            },
            "models": [
                { "id": "a", "displayName": "A" },
                { "id": "b", "displayName": "B", "description": "second" }
            ]
        }"#;
        let p = load(json, "module.exports.default = {};");
        let ms = p.available_models();
        assert_eq!(ms.len(), 2);
        assert_eq!(ms[1].description.as_deref(), Some("second"));
    }

    #[test]
    fn install_command_resolved_per_platform() {
        let json = r#"{
            "id": "ic",
            "version": "0.1.0",
            "displayName": "IC",
            "spawn": {
                "readinessStrategy": { "kind": "fixed_delay", "durationMs": 0 },
                "taskDelivery": "pty_input",
                "verifySubmit": { "kind": "none" }
            },
            "installer": {
                "macos": "brew install ic",
                "linux": "snap install ic"
            }
        }"#;
        let p = load(json, "module.exports.default = {};");
        assert_eq!(p.install_command("macos").as_deref(), Some("brew install ic"));
        assert_eq!(p.install_command("linux").as_deref(), Some("snap install ic"));
        assert_eq!(p.install_command("windows"), None);
    }

    #[test]
    fn detect_events_returns_permission_request_via_plugin() {
        let js = r#"
            module.exports.default = {
                detectEvents(text) {
                    return text.includes('ALLOW?') ? ['permission_request'] : [];
                }
            };
        "#;
        let p = load(MIN_MANIFEST, js);
        assert_eq!(p.detect_events("plain text").len(), 0);
        let evts = p.detect_events("Do you ALLOW?");
        assert_eq!(evts.len(), 1);
    }

    #[test]
    fn build_resume_command_returns_none_when_plugin_missing() {
        let p = load(MIN_MANIFEST, "module.exports.default = {};");
        assert_eq!(p.build_resume_command("sid", true), None);
    }

    #[test]
    fn build_resume_command_dispatches_to_js_with_two_args() {
        let js = r#"
            module.exports.default = {
                buildResumeCommand(sid, skip) {
                    return 'demo --resume ' + sid + (skip ? ' --yolo' : '');
                }
            };
        "#;
        let p = load(MIN_MANIFEST, js);
        assert_eq!(
            p.build_resume_command("S1", true),
            Some("demo --resume S1 --yolo".into())
        );
        assert_eq!(
            p.build_resume_command("S1", false),
            Some("demo --resume S1".into())
        );
    }

    #[test]
    fn _unused_import_guard() {
        // Keep the encode helper exercised so the test suite reflects its
        // production use in sessions_dir().
        let _ = encode_cwd_dashed("/home/x/proj");
        let _ = json!(null);
    }
}
