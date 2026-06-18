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
    /// Minimum CodeTerm version required; gated by the plugin CLI on install/update.
    #[serde(default)]
    pub min_codeterm: Option<String>,
    /// Plugin/host contract version this drop-in targets. Third-party plugins
    /// must declare a value this build supports (`host_api::HOST_API_VERSION`)
    /// or they are skipped at load instead of registered against an interface
    /// they don't conform to (#16). First-party bundled plugins are exempt.
    #[serde(default)]
    pub host_api: Option<u32>,

    // Documentation surface (shown in the Extensions detail pane). `readme` is a
    // relative markdown path the snapshot loads into a bounded field.
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub readme: Option<String>,
    /// Relative path to an agent-facing doc (e.g. `AGENT.md`) the `get plugin`
    /// surface returns so an agent can install + self-configure the plugin
    /// (§1.7). Plain markdown; optional.
    #[serde(default)]
    pub agent_doc: Option<String>,

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

    // Spawn semantics. Required for agent providers; capability-only plugins
    // (transcriber, secret backend) leave it defaulted.
    #[serde(default)]
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

    /// Which core capability traits this plugin implements. Absent → treated as
    /// an agent provider (back-compat for every existing bundled plugin, which
    /// predates this field). A plugin that declares any capability gets exactly
    /// the ones it lists.
    #[serde(default = "default_capabilities")]
    pub capabilities: Capabilities,

    /// Path (relative to the plugin dir) to a `SettingField[]` JSON schema that
    /// the Extensions UI renders as the plugin's settings panel, writing values
    /// into `config.plugins.<id>`. Optional.
    #[serde(default)]
    pub settings_schema: Option<String>,

    /// The plugin's capability cannot hot-activate — it needs a CodeTerm reload
    /// to take effect (§1.8). When a plugin declaring this is *installed/enabled
    /// after daemon start*, the watcher marks it `pending_reload` instead of
    /// registering its capability live (which wouldn't work). Full daemon start
    /// activates it normally. Surfaced via `get plugin` / install `--json` and a
    /// "Reload required" banner in the Extensions UI.
    #[serde(default)]
    pub requires_reload: bool,

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

/// Manifests without a `capabilities` block are agent providers (every plugin
/// shipped before capabilities existed).
fn default_capabilities() -> Capabilities {
    Capabilities {
        agent_provider: true,
        secret_backend: false,
        transcriber: false,
        glance_view: false,
        status_bubble: false,
        view: false,
        chat_backend: false,
        chat_preprocessor: ChatPreprocessorCap::default(),
    }
}

/// `chatPreprocessor` capability declaration. Authors write either a bare bool
/// (`true`) or an object naming the pipeline slot it grabs (`{ "match": "text" }`
/// — the §6 conflict-guard slot). Both forms mean "enabled"; the object also
/// carries the declared slot so the conflict guard knows which slot it claims.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChatPreprocessorCap {
    pub enabled: bool,
    /// Declared pipeline slot (`match`). `None` ⇒ the text slot, the only slot a
    /// preprocessor may grab today (the SDK fixes `match: "text"`).
    pub match_slot: Option<String>,
}

impl ChatPreprocessorCap {
    pub fn enabled(&self) -> bool {
        self.enabled
    }
}

impl<'de> Deserialize<'de> for ChatPreprocessorCap {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Flag(bool),
            Obj {
                #[serde(default, rename = "match")]
                match_slot: Option<String>,
            },
        }
        Ok(match Raw::deserialize(d)? {
            Raw::Flag(enabled) => Self {
                enabled,
                match_slot: None,
            },
            Raw::Obj { match_slot } => Self {
                enabled: true,
                match_slot,
            },
        })
    }
}

impl Serialize for ChatPreprocessorCap {
    /// Symmetric with `Deserialize`: a slot ⇒ object, otherwise the bare bool —
    /// so a disabled cap round-trips to `false`, never silently re-enabling.
    fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match &self.match_slot {
            Some(slot) if self.enabled => {
                use serde::ser::SerializeMap;
                let mut m = s.serialize_map(Some(1))?;
                m.serialize_entry("match", slot)?;
                m.end()
            }
            _ => s.serialize_bool(self.enabled),
        }
    }
}

/// The core capability traits a plugin can implement. Drives which adapters the
/// loader builds (`JsProvider` / `JsSecretBackend` / `JsTranscriber` /
/// `JsGlanceProvider` / `JsBubbleProvider`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Capabilities {
    #[serde(default)]
    pub agent_provider: bool,
    #[serde(default)]
    pub secret_backend: bool,
    #[serde(default)]
    pub transcriber: bool,
    /// Renders a read-only "glance" popover (ViewNode tree) when pinned.
    #[serde(default)]
    pub glance_view: bool,
    /// Contributes a status pill into the pane-footer bubble row.
    #[serde(default)]
    pub status_bubble: bool,
    /// Ships a full HTML/JS/CSS UI (`ui/index.html`), rendered in a sandboxed
    /// iframe (VS Code/Figma webview model). Talks to the host only through the
    /// permission-gated `ct.invoke` postMessage bridge → the plugin's QuickJS
    /// exports. Behind a loud install grant.
    #[serde(default)]
    pub view: bool,
    /// Drives a non-PTY pane as a full conversational backend (e.g. LM Studio).
    /// The pane routes send/read through the plugin instead of a terminal.
    #[serde(default)]
    pub chat_backend: bool,
    /// Send-time text stage of the outbound pipeline (`chatPreprocess`). Matches
    /// final text and augments it (mem recall / worker / Groq) before transport.
    /// Bool or `{ match }` object (the §6 slot).
    #[serde(default)]
    pub chat_preprocessor: ChatPreprocessorCap,
}

/// Hosts/origins a plugin may reach via `host.fetch`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NetworkPermission {
    #[serde(default)]
    pub allow: Vec<String>,
}

/// Binaries a plugin may run via `host.exec`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SubprocessPermission {
    #[serde(default)]
    pub allow: Vec<String>,
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
    /// Plugin exports `parseInteractiveEvents` — host calls it to extract
    /// interactive prompts. When false the native fallback handles it.
    #[serde(default)]
    pub interactive_events: bool,
    /// TUI command that prints the account usage screen (e.g. claude `/usage`,
    /// codex `/status`). Pairs with the plugin's `parseUsage` export; the
    /// usage poller skips providers without both.
    #[serde(default)]
    pub usage_probe_command: Option<String>,
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
    /// `host.fetch` host allowlist.
    #[serde(default)]
    pub network: NetworkPermission,
    /// `host.exec` binary allowlist.
    #[serde(default)]
    pub subprocess: SubprocessPermission,
    /// Whether the plugin may use its per-plugin secret bucket (`host.secret*`).
    #[serde(default)]
    pub secrets: bool,
    /// Whether a spawned agent may route `codeterm plugin <id> <verb>` to this
    /// plugin's `onAgentCommand` (the inverse seam — plan §1.5 F5). Canonical
    /// snake_case (the casing grants.yaml / snapshot / frontend all use);
    /// camelCase accepted as an alias.
    #[serde(default, rename = "agent_commands", alias = "agentCommands")]
    pub agent_commands: bool,
    /// Whether the plugin may create/manage workspaces via `host.workspace*`
    /// (enforced perm — plan §1.10 P3).
    #[serde(default, rename = "manage_workspaces", alias = "manageWorkspaces")]
    pub manage_workspaces: bool,
    /// Whether the plugin may spawn/drive our agents via `host.agent*`
    /// (enforced perm — plan §1.10 P3).
    #[serde(default, rename = "spawn_agents", alias = "spawnAgents")]
    pub spawn_agents: bool,
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
    fn parse_chat_preprocessor_object_carries_match() {
        // §6: chatPreprocessor authored as an OBJECT declaring its slot. The
        // host must accept this (not only the bare bool) or the plugin never
        // loads.
        let json = r#"{
            "id": "chat-preprocessor",
            "version": "0.1.0",
            "displayName": "Chat Preprocessor",
            "capabilities": { "chatPreprocessor": { "match": "text" } }
        }"#;
        let m: PluginManifest = serde_json::from_str(json).expect("parse object form");
        assert!(m.capabilities.chat_preprocessor.enabled());
        assert_eq!(
            m.capabilities.chat_preprocessor.match_slot.as_deref(),
            Some("text"),
            "match slot carried into the capability for the conflict guard"
        );
    }

    #[test]
    fn parse_chat_preprocessor_bool_still_works() {
        let json = r#"{
            "id": "p",
            "version": "0.1.0",
            "displayName": "P",
            "capabilities": { "chatPreprocessor": true }
        }"#;
        let m: PluginManifest = serde_json::from_str(json).expect("parse bool form");
        assert!(m.capabilities.chat_preprocessor.enabled());
        assert_eq!(m.capabilities.chat_preprocessor.match_slot, None);
    }

    #[test]
    fn chat_preprocessor_round_trips_symmetrically() {
        // Disabled must not silently flip to enabled on re-parse (serialize as
        // bool when no slot, object when a slot is declared).
        let cap = Capabilities::default();
        let v = serde_json::to_value(&cap.chat_preprocessor).unwrap();
        assert_eq!(v, serde_json::json!(false));
        let back: ChatPreprocessorCap = serde_json::from_value(v).unwrap();
        assert!(!back.enabled());
    }

    #[test]
    fn parse_agent_doc_field() {
        // §1.7 agent-doc: codeterm-npc declares `agentDoc`; deny_unknown_fields
        // would reject it without the schema entry → plugin fails to load.
        let json = r#"{
            "id": "codeterm-npc",
            "version": "0.1.0",
            "displayName": "Dread NPCs",
            "agentDoc": "AGENT.md",
            "capabilities": { "chatBackend": true }
        }"#;
        let m: PluginManifest = serde_json::from_str(json).expect("parse agentDoc");
        assert_eq!(m.agent_doc.as_deref(), Some("AGENT.md"));
    }

    #[test]
    fn parse_snake_case_new_permissions() {
        // codeterm-npc authors the powerful grants in snake_case — the casing the
        // grant store / snapshot / frontend all use. The manifest must accept it.
        let json = r#"{
            "id": "codeterm-npc",
            "version": "0.1.0",
            "displayName": "Dread NPCs",
            "permissions": {
                "manage_workspaces": true,
                "spawn_agents": true,
                "agent_commands": true,
                "secrets": true
            }
        }"#;
        let m: PluginManifest = serde_json::from_str(json).expect("parse snake perms");
        assert!(m.permissions.manage_workspaces);
        assert!(m.permissions.spawn_agents);
        assert!(m.permissions.agent_commands);
        assert!(m.permissions.secrets);
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
