//! `PluginManifest` — the typed shape of `plugin.json`.
//!
//! Everything a provider plugin needs to declare statically (id,
//! display name, icons, models, spawn semantics, file paths) lives
//! here as plain data. Anything that needs *logic* (parsing PTY
//! output, building shell commands from arbitrary inputs) lives in
//! `plugin.js` and gets called via `PluginVm`.
//!
//! The Rust core never hardcodes provider names — it reads manifests
//! at startup and instantiates one generic `JsProvider` per plugin.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::agent::provider::{
    ModelInfo, ReadinessStrategy, SessionIdInjection, SettingField, TaskDelivery,
    VerifySubmitStrategy,
};

/// Top-level `plugin.json` document.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PluginManifest {
    pub id: String,
    pub version: String,
    pub display_name: String,
    #[serde(default)]
    pub engine: Option<String>,

    // Identity / UI
    #[serde(default)]
    pub icon_hint: Option<String>,
    #[serde(default)]
    pub icon_color: Option<String>,
    #[serde(default)]
    pub icon_svg: Option<String>,
    #[serde(default)]
    pub binary_name: Option<String>,
    #[serde(default)]
    pub cli_binary: Option<String>,

    // Spawn semantics
    pub spawn: SpawnConfig,

    // Optional features (most providers leave these omitted)
    #[serde(default)]
    pub sessions: Option<SessionsConfig>,
    #[serde(default)]
    pub integration: Option<IntegrationConfig>,
    #[serde(default)]
    pub rule: Option<RuleConfig>,
    #[serde(default)]
    pub skills: Option<SkillsConfig>,
    #[serde(default)]
    pub usage: Option<UsageConfig>,

    // Models — plugin.json uses camelCase, ModelInfo (wire format) is snake_case, so we deserialize through a local wrapper.
    #[serde(default)]
    pub models: Vec<SerdeModelInfo>,
    #[serde(default)]
    pub default_model: Option<String>,

    // Per-platform installer (macos/linux/windows)
    #[serde(default)]
    pub installer: std::collections::HashMap<String, String>,

    // Slash commands / control literals
    #[serde(default)]
    pub commands: CommandsConfig,

    // Plugin capabilities (mostly informational; transport layer enforces permissions where it matters)
    #[serde(default)]
    pub permissions: PermissionsConfig,

    // Path of plugin.js relative to the manifest, defaults to "dist/plugin.js".
    #[serde(default = "default_entry")]
    pub entry: String,

    /// Absolute path the manifest was loaded from. Filled by the loader,
    /// not present in JSON.
    #[serde(skip)]
    pub source_path: Option<PathBuf>,
}

fn default_entry() -> String {
    "dist/plugin.js".to_string()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SpawnConfig {
    pub readiness_strategy: SerdeReadinessStrategy,
    pub task_delivery: SerdeTaskDelivery,
    #[serde(default)]
    pub task_input_warmup_ms: u64,
    pub verify_submit: SerdeVerifySubmit,
    #[serde(default)]
    pub session_id_injection: SerdeSessionIdInjection,
    #[serde(default)]
    pub status_bar_rows: u16,
    #[serde(default)]
    pub preserves_context_on_resume: bool,
    #[serde(default)]
    pub supports_lazy_skills: bool,
    #[serde(default)]
    pub has_hooks: bool,
}

/// Mirror of `crate::agent::provider::ReadinessStrategy` with snake_case
/// serde shape suitable for plugin.json authors.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
#[allow(clippy::enum_variant_names)]
pub enum SerdeReadinessStrategy {
    PollShellPrompt { timeout_secs: u64 },
    FixedDelay { duration_ms: u64 },
}

impl Default for SerdeReadinessStrategy {
    fn default() -> Self {
        Self::PollShellPrompt { timeout_secs: 30 }
    }
}

impl From<&SerdeReadinessStrategy> for ReadinessStrategy {
    fn from(s: &SerdeReadinessStrategy) -> Self {
        match s {
            SerdeReadinessStrategy::PollShellPrompt { timeout_secs } => Self::PollShellPrompt {
                timeout: std::time::Duration::from_secs(*timeout_secs),
            },
            SerdeReadinessStrategy::FixedDelay { duration_ms } => Self::FixedDelay {
                duration: std::time::Duration::from_millis(*duration_ms),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SerdeTaskDelivery {
    InlineFlag,
    PtyInput,
}

impl Default for SerdeTaskDelivery {
    fn default() -> Self {
        Self::PtyInput
    }
}

impl From<SerdeTaskDelivery> for TaskDelivery {
    fn from(s: SerdeTaskDelivery) -> Self {
        match s {
            SerdeTaskDelivery::InlineFlag => Self::InlineFlag,
            SerdeTaskDelivery::PtyInput => Self::PtyInput,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
#[allow(clippy::enum_variant_names)]
pub enum SerdeVerifySubmit {
    ShellPrompt {
        timeout_secs: u64,
    },
    TuiInputDrained {
        timeout_secs: u64,
        idle_after_drain_ms: u64,
    },
    None,
}

impl Default for SerdeVerifySubmit {
    fn default() -> Self {
        Self::ShellPrompt { timeout_secs: 5 }
    }
}

impl From<&SerdeVerifySubmit> for VerifySubmitStrategy {
    fn from(s: &SerdeVerifySubmit) -> Self {
        match s {
            SerdeVerifySubmit::ShellPrompt { timeout_secs } => Self::ShellPrompt {
                timeout: std::time::Duration::from_secs(*timeout_secs),
            },
            SerdeVerifySubmit::TuiInputDrained {
                timeout_secs,
                idle_after_drain_ms,
            } => Self::TuiInputDrained {
                timeout: std::time::Duration::from_secs(*timeout_secs),
                idle_after_drain: std::time::Duration::from_millis(*idle_after_drain_ms),
            },
            SerdeVerifySubmit::None => Self::None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SerdeSessionIdInjection {
    #[default]
    None,
    Fresh,
    FreshAndFork,
}

impl From<SerdeSessionIdInjection> for SessionIdInjection {
    fn from(s: SerdeSessionIdInjection) -> Self {
        match s {
            SerdeSessionIdInjection::None => Self::None,
            SerdeSessionIdInjection::Fresh => Self::Fresh,
            SerdeSessionIdInjection::FreshAndFork => Self::FreshAndFork,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SessionsConfig {
    /// Filesystem root the provider writes session files into. May
    /// start with `~/` (expanded to home dir). E.g.
    /// `~/.claude/projects`.
    pub root_dir: String,
    /// How a working-directory path is mapped into a subdirectory name
    /// under `root_dir`. Claude encodes `/Users/foo/code` as
    /// `-Users-foo-code`.
    #[serde(default)]
    pub cwd_encoding: CwdEncoding,
    /// File extension session files use (`jsonl`, `json`, etc).
    #[serde(default = "default_session_ext")]
    pub file_extension: String,
}

fn default_session_ext() -> String {
    "jsonl".to_string()
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CwdEncoding {
    /// Slashes (and backslashes) become dashes, leading separator
    /// kept: `/Users/foo/code` → `-Users-foo-code`.
    #[default]
    DashEncoded,
    /// Path used as-is (rare).
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct IntegrationConfig {
    /// What kind of integration to install. Each kind has a fixed
    /// Rust implementation; plugins declare which one applies.
    pub kind: IntegrationKind,
    /// Where the integration is stored (e.g. `~/.claude/settings.json`).
    pub settings_path: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationKind {
    /// Claude-style: edit a JSON file's `hooks` object with curl-back
    /// commands.
    ClaudeSettingsJson,
    /// No installable integration. PTY parser does it all.
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RuleConfig {
    /// Where the codeterm-rule markdown file lives.
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SkillsConfig {
    /// Where plugin-contributed skill files get written.
    pub dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct UsageConfig {
    /// Slash-command that probes usage in the live TUI (e.g. `/usage`).
    pub probe_command: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CommandsConfig {
    #[serde(default)]
    pub compact: Option<String>,
    #[serde(default)]
    pub exit: Option<String>,
    /// Raw key bytes the host should send to interrupt the agent's
    /// current operation (NOT a clean exit — think Esc / Ctrl+C while
    /// the model is generating). Examples: `""` for Esc,
    /// `""` for Ctrl+C.
    #[serde(default)]
    pub stop_key: Option<String>,
    /// Format string with `{model}` placeholder.
    #[serde(default)]
    pub model_switch_format: Option<String>,
    #[serde(default)]
    pub auto_approve_flags: Vec<String>,
    /// Each element may contain `{model}` placeholder.
    #[serde(default)]
    pub model_launch_args_format: Vec<String>,
}

/// CamelCase-friendly mirror of `ModelInfo` for plugin.json authors.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SerdeModelInfo {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub description: Option<String>,
}

impl From<&SerdeModelInfo> for ModelInfo {
    fn from(s: &SerdeModelInfo) -> Self {
        Self {
            id: s.id.clone(),
            display_name: s.display_name.clone(),
            description: s.description.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PermissionsConfig {
    #[serde(default)]
    pub transport_events: Vec<String>,
    #[serde(default)]
    pub send_text: bool,
}

impl PluginManifest {
    /// Parse a manifest from a `plugin.json` file on disk.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text =
            std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let mut m: PluginManifest =
            serde_json::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))?;
        m.source_path = Some(path.to_path_buf());
        Ok(m)
    }

    /// Directory the manifest lives in — `plugin.js` resolves relative
    /// to this.
    pub fn dir(&self) -> Option<PathBuf> {
        self.source_path
            .as_ref()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
    }

    /// Absolute path to the bundle's `plugin.js`.
    pub fn entry_path(&self) -> Option<PathBuf> {
        self.dir().map(|d| d.join(&self.entry))
    }

    /// Build the settings schema fields contributed by manifest data.
    /// Plugin's settingsSchema() may add more.
    pub fn applies_to_platform(&self, platform: &str) -> bool {
        let _ = platform;
        true // platform-targeted plugins are out of scope for v1
    }

    /// Expand a `~/...` path to the user's home directory.
    pub fn expand_home(path: &str) -> Option<PathBuf> {
        if let Some(rest) = path.strip_prefix("~/") {
            dirs::home_dir().map(|h| h.join(rest))
        } else if path == "~" {
            dirs::home_dir()
        } else {
            Some(PathBuf::from(path))
        }
    }
}

/// Build the standard `SettingField` sections every provider with a
/// session-resume + model story gets for free. Plugin's `settingsSchema()`
/// adds provider-specific sections on top.
pub fn manifest_settings_sections(m: &PluginManifest) -> Vec<SettingField> {
    let mut out = Vec::new();
    if let Some(flag) = m.commands.auto_approve_flags.first() {
        out.push(crate::agent::provider::session_resume_section(
            &format!(
                "Automatically resume {} sessions when CodeTerm restarts.",
                m.display_name
            ),
            flag,
        ));
    }
    if !m.models.is_empty() {
        out.push(crate::agent::provider::model_select_section(
            &format!("Default model for new {} sessions.", m.display_name),
            m.models.iter().map(ModelInfo::from).collect(),
            m.default_model.as_deref(),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_manifest() {
        let json = r#"{
            "id": "demo",
            "version": "0.1.0",
            "displayName": "Demo CLI",
            "spawn": {
                "readinessStrategy": { "kind": "fixed_delay", "durationMs": 100 },
                "taskDelivery": "pty_input",
                "verifySubmit": { "kind": "none" }
            }
        }"#;
        let m: PluginManifest = serde_json::from_str(json).expect("parse");
        assert_eq!(m.id, "demo");
        assert_eq!(m.display_name, "Demo CLI");
        assert_eq!(m.entry, "dist/plugin.js");
    }

    #[test]
    fn parse_full_manifest() {
        let json = r##"{
            "id": "claude",
            "version": "0.1.0",
            "displayName": "Claude Code",
            "iconHint": "✳",
            "iconColor": "#d97757",
            "binaryName": "claude",
            "cliBinary": "claude",
            "spawn": {
                "readinessStrategy": { "kind": "poll_shell_prompt", "timeoutSecs": 30 },
                "taskDelivery": "inline_flag",
                "taskInputWarmupMs": 0,
                "verifySubmit": { "kind": "tui_input_drained", "timeoutSecs": 15, "idleAfterDrainMs": 800 },
                "sessionIdInjection": "fresh_and_fork",
                "statusBarRows": 3,
                "preservesContextOnResume": true,
                "supportsLazySkills": true,
                "hasHooks": true
            },
            "sessions": {
                "rootDir": "~/.claude/projects",
                "cwdEncoding": "dash_encoded",
                "fileExtension": "jsonl"
            },
            "integration": {
                "kind": "claude_settings_json",
                "settingsPath": "~/.claude/settings.json"
            },
            "rule": {
                "path": "~/.claude/rules/codeterm.md"
            },
            "models": [
                { "id": "sonnet", "displayName": "Claude Sonnet 4.6", "description": "Best for everyday tasks." }
            ],
            "defaultModel": "sonnet",
            "installer": {
                "macos": "curl -fsSL https://claude.ai/install.sh | bash"
            },
            "commands": {
                "compact": "/compact",
                "exit": "/exit",
                "modelSwitchFormat": "/model {model}",
                "autoApproveFlags": ["--dangerously-skip-permissions"],
                "modelLaunchArgsFormat": ["--model", "{model}"]
            },
            "permissions": {
                "transportEvents": ["pty", "events"],
                "sendText": true
            }
        }"##;
        let m: PluginManifest = serde_json::from_str(json).expect("parse");
        assert_eq!(m.id, "claude");
        assert_eq!(m.icon_hint.as_deref(), Some("✳"));
        assert_eq!(m.models.len(), 1);
        assert_eq!(m.default_model.as_deref(), Some("sonnet"));
        assert_eq!(
            m.commands.auto_approve_flags,
            vec!["--dangerously-skip-permissions"]
        );
        assert!(m.spawn.has_hooks);
        assert_eq!(m.spawn.status_bar_rows, 3);
        assert!(matches!(
            m.spawn.verify_submit,
            SerdeVerifySubmit::TuiInputDrained { .. }
        ));
        assert!(matches!(
            m.spawn.session_id_injection,
            SerdeSessionIdInjection::FreshAndFork
        ));
    }

    #[test]
    fn expand_home_handles_tilde() {
        if let Some(home) = dirs::home_dir() {
            assert_eq!(
                PluginManifest::expand_home("~/foo/bar"),
                Some(home.join("foo/bar"))
            );
            assert_eq!(PluginManifest::expand_home("~"), Some(home));
        }
        assert_eq!(
            PluginManifest::expand_home("/abs/path"),
            Some(PathBuf::from("/abs/path"))
        );
    }

    #[test]
    fn readiness_conversion() {
        let s = SerdeReadinessStrategy::PollShellPrompt { timeout_secs: 10 };
        let r: ReadinessStrategy = (&s).into();
        match r {
            ReadinessStrategy::PollShellPrompt { timeout } => {
                assert_eq!(timeout.as_secs(), 10);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn verify_submit_conversion() {
        let s = SerdeVerifySubmit::TuiInputDrained {
            timeout_secs: 15,
            idle_after_drain_ms: 800,
        };
        let v: VerifySubmitStrategy = (&s).into();
        match v {
            VerifySubmitStrategy::TuiInputDrained {
                timeout,
                idle_after_drain,
            } => {
                assert_eq!(timeout.as_secs(), 15);
                assert_eq!(idle_after_drain.as_millis(), 800);
            }
            _ => panic!(),
        }
    }
}
