use serde::{Deserialize, Serialize};

use crate::agent::provider::{
    ModelInfo, ReadinessStrategy, SessionIdInjection, TaskDelivery, VerifySubmitStrategy,
};

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
